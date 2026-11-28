use evanescent::environment::{StackReason, StackStatus};
use k8s_openapi::api::rbac::v1::{RoleBinding, RoleRef, Subject};
use kube::{
    Api, Client,
    api::{ObjectMeta, PostParams},
};
use thiserror::Error;

use crate::{catalog, helm};

const ROLE: &str = "evanescent-helm";

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot manage stack resources: {0}")]
    Kubernetes(#[from] kube::Error),
    #[error("the Helm role binding has an unexpected role or subject")]
    RoleBindingConflict,
    #[error(transparent)]
    Catalog(#[from] catalog::Error),
    #[error(transparent)]
    Helm(#[from] helm::Error),
}

impl Error {
    pub fn reason(&self) -> StackReason {
        match self {
            Self::Helm(helm::Error::Pending) => StackReason::ReleasePending,
            Self::Catalog(_) => StackReason::TemplateRejected,
            Self::Helm(helm::Error::UnmanagedRelease) => StackReason::UnmanagedRelease,
            _ => StackReason::ReconcileFailed,
        }
    }
}

pub async fn reconcile(
    client: Client,
    namespace: &str,
    control_namespace: &str,
    release: catalog::Release<'_>,
    generation: i64,
) -> Result<StackStatus, Error> {
    ensure_access(client, namespace, control_namespace).await?;
    let mut args = Vec::new();
    if release.plain_http {
        args.push("--plain-http");
    }
    for value in &release.overrides {
        args.extend(["--set-json", value.as_str()]);
    }
    let revision = helm::deploy(namespace, release.chart, &release.description, &args).await?;
    Ok(StackStatus {
        observed_generation: generation,
        ready: true,
        reason: StackReason::ReleaseReady,
        release_revision: Some(revision),
    })
}

/// Helm access is created before any release, including under older controllers.
pub async fn has_access(client: Client, namespace: &str) -> Result<bool, kube::Error> {
    let bindings: Api<RoleBinding> = Api::namespaced(client, namespace);
    Ok(bindings.get_opt(ROLE).await?.is_some())
}

pub async fn uninstall(
    client: Client,
    namespace: &str,
    control_namespace: &str,
) -> Result<(), Error> {
    ensure_access(client, namespace, control_namespace).await?;
    helm::uninstall(namespace).await?;
    Ok(())
}

async fn ensure_access(
    client: Client,
    namespace: &str,
    control_namespace: &str,
) -> Result<(), Error> {
    let bindings: Api<RoleBinding> = Api::namespaced(client, namespace);
    let role_ref = RoleRef {
        api_group: Some("rbac.authorization.k8s.io".to_owned()),
        kind: "ClusterRole".to_owned(),
        name: ROLE.to_owned(),
    };
    let subjects = Some(vec![Subject {
        kind: "ServiceAccount".to_owned(),
        name: "evanescent".to_owned(),
        namespace: Some(control_namespace.to_owned()),
        ..Subject::default()
    }]);

    if let Some(binding) = bindings.get_opt(ROLE).await? {
        if binding.role_ref != role_ref || binding.subjects != subjects {
            return Err(Error::RoleBindingConflict);
        }
    } else {
        let binding = RoleBinding {
            metadata: ObjectMeta {
                name: Some(ROLE.to_owned()),
                ..ObjectMeta::default()
            },
            role_ref,
            subjects,
        };
        bindings.create(&PostParams::default(), &binding).await?;
    }
    Ok(())
}
