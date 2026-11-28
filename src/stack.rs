use evanescent::environment::{PostgrestVersion, StackReason, StackStatus};
use k8s_openapi::api::{
    apps::v1::{Deployment, StatefulSet},
    rbac::v1::{RoleBinding, RoleRef, Subject},
};
use kube::{
    Api, Client,
    api::{ObjectMeta, PostParams},
};
use thiserror::Error;

use crate::{catalog, helm};

const BUNDLED_CHART: &str = "/opt/evanescent/charts/api-postgres";

const CHART_VERSION: &str = "0.1.0";
const ROLE: &str = "evanescent-helm";
const API_DEPLOYMENT: &str = "api";
const DATABASE_STATEFUL_SET: &str = "postgres";

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
            _ => StackReason::ReconcileFailed,
        }
    }
}

pub async fn reconcile(
    client: Client,
    namespace: &str,
    control_namespace: &str,
    version: PostgrestVersion,
    generation: i64,
) -> Result<StackStatus, Error> {
    ensure_access(client.clone(), namespace, control_namespace).await?;
    let image = api_image(version);
    let desired = format!("evanescent/api-postgres/{CHART_VERSION}/{image}");
    let values = format!("api.image={image}");
    let revision = helm::deploy(
        namespace,
        BUNDLED_CHART,
        &desired,
        &["--set-string", &values],
    )
    .await?;
    let ready = workloads_ready(client, namespace).await?;
    Ok(StackStatus {
        observed_generation: generation,
        ready,
        reason: if ready {
            StackReason::ReleaseReady
        } else {
            StackReason::WorkloadsNotReady
        },
        release_revision: Some(revision),
    })
}

pub async fn reconcile_chart(
    client: Client,
    namespace: &str,
    control_namespace: &str,
    release: catalog::Release<'_>,
    generation: i64,
) -> Result<StackStatus, Error> {
    ensure_access(client, namespace, control_namespace).await?;
    let mut args = vec!["--reset-values", "--wait-for-jobs"];
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

async fn workloads_ready(client: Client, namespace: &str) -> Result<bool, kube::Error> {
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    let stateful_sets: Api<StatefulSet> = Api::namespaced(client, namespace);
    let Some(api) = deployments.get_opt(API_DEPLOYMENT).await? else {
        return Ok(false);
    };
    let Some(database) = stateful_sets.get_opt(DATABASE_STATEFUL_SET).await? else {
        return Ok(false);
    };
    Ok(api.status.is_some_and(|status| {
        status.observed_generation == api.metadata.generation
            && status.updated_replicas == Some(1)
            && status.available_replicas == Some(1)
            && status.replicas == Some(1)
    }) && database.status.is_some_and(|status| {
        status.observed_generation == database.metadata.generation
            && status.ready_replicas == Some(1)
            && status.current_revision == status.update_revision
    }))
}

fn api_image(version: PostgrestVersion) -> &'static str {
    match version {
        PostgrestVersion::V16_5 => {
            "postgrest/postgrest:v16.5@sha256:1347d7552b88f17832912c06afbf7c43209af6eeeb60834b04aae12c9bd9d5cc"
        }
        PostgrestVersion::V16_6 => {
            "postgrest/postgrest:v16.6@sha256:5ce030c153500fe90ba157da4da8ba99ab51d3bc7b95e071bd0b544b67a632f7"
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::CHART_VERSION;

    #[test]
    fn release_identity_uses_the_bundled_chart_version() {
        let chart: Value =
            serde_saphyr::from_str(include_str!("../charts/api-postgres/Chart.yaml")).unwrap();
        assert_eq!(chart["version"], CHART_VERSION);
    }
}
