use std::{collections::BTreeMap, sync::Arc, time::Duration};

use evanescent::environment::{EphemeralEnvironment, EphemeralEnvironmentStatus, StackStatus};
use futures::StreamExt;
use k8s_openapi::api::core::v1::Namespace;
use kube::{
    Api, Client, ResourceExt,
    api::{DeleteParams, ObjectMeta, Patch, PatchParams, PostParams, Preconditions},
    runtime::{
        Controller,
        controller::{Action, Config, Error as ControllerError},
        finalizer::{self, Event, finalizer},
        watcher,
    },
};
use serde_json::json;
use thiserror::Error;

use crate::stack;

const CONTROL_NAMESPACE: &str = "evanescent-system";
const FINALIZER: &str = "evanescent.caelurium.github.io/namespace-cleanup";
const OWNER_LABEL: &str = "evanescent.caelurium.github.io/environment-uid";

struct Context {
    client: Client,
    environments: Api<EphemeralEnvironment>,
    namespaces: Api<Namespace>,
}

#[derive(Debug, Error)]
enum ReconcileError {
    #[error("Kubernetes API request failed: {0}")]
    Kubernetes(#[from] kube::Error),
    #[error("{0} has no UID")]
    MissingUid(String),
    #[error("namespace {0} is not owned by this ephemeral environment")]
    OwnershipConflict(String),
    #[error("waiting for namespace deletion")]
    NamespaceDeleting,
    #[error(transparent)]
    Stack(#[from] stack::Error),
}

pub async fn run(client: Client) {
    let ctx = Arc::new(Context {
        environments: Api::namespaced(client.clone(), CONTROL_NAMESPACE),
        namespaces: Api::all(client.clone()),
        client,
    });

    tracing::info!(
        namespace = CONTROL_NAMESPACE,
        "watching ephemeral environments"
    );
    Controller::new(ctx.environments.clone(), watcher::Config::default())
        .with_config(Config::default().concurrency(4))
        .shutdown_on_signal()
        .run(reconcile, error_policy, ctx)
        .for_each(|result| async {
            match result {
                Ok(_)
                | Err(ControllerError::ReconcilerFailed(..) | ControllerError::ObjectNotFound(_)) =>
                    {}
                Err(error) => tracing::warn!(%error, "controller stream error"),
            }
        })
        .await;
}

async fn reconcile(
    env: Arc<EphemeralEnvironment>,
    ctx: Arc<Context>,
) -> Result<Action, finalizer::Error<ReconcileError>> {
    finalizer(&ctx.environments, FINALIZER, env, |event| async {
        match event {
            Event::Apply(env) => apply(&env, &ctx).await,
            Event::Cleanup(env) => cleanup(&env, &ctx).await,
        }
    })
    .await
}

async fn apply(env: &EphemeralEnvironment, ctx: &Context) -> Result<Action, ReconcileError> {
    let uid = environment_uid(env)?;
    let name = namespace_name(uid);
    match ctx.namespaces.get_opt(&name).await? {
        Some(ns) if !owns_namespace(&ns, uid) => {
            return Err(ReconcileError::OwnershipConflict(name));
        }
        Some(ns) if ns.metadata.deletion_timestamp.is_some() => {
            return Ok(Action::requeue(Duration::from_secs(2)));
        }
        Some(_) => {}
        None => {
            let ns = Namespace {
                metadata: ObjectMeta {
                    name: Some(name.clone()),
                    labels: Some(BTreeMap::from([(OWNER_LABEL.to_owned(), uid.to_owned())])),
                    ..ObjectMeta::default()
                },
                ..Namespace::default()
            };
            ctx.namespaces.create(&PostParams::default(), &ns).await?;
            tracing::info!(namespace = name, "created environment namespace");
        }
    }

    if env.status.as_ref().map(|status| status.namespace.as_str()) != Some(name.as_str()) {
        update_status(env, ctx, name, None).await?;
        return Ok(Action::requeue(Duration::ZERO));
    }

    if let Some(spec) = &env.spec.api_postgres {
        let generation = env.metadata.generation.unwrap_or_default();
        match stack::reconcile(
            ctx.client.clone(),
            &name,
            CONTROL_NAMESPACE,
            spec.api_version,
            generation,
        )
        .await
        {
            Ok(status) => update_status(env, ctx, name, Some(status)).await?,
            Err(error) => {
                let status = StackStatus {
                    observed_generation: generation,
                    ready: false,
                    reason: error.reason(),
                    release_revision: None,
                };
                update_status(env, ctx, name, Some(status)).await?;
                return Err(error.into());
            }
        }
    }

    // Check for out-of-band namespace deletion without a cluster-wide watch.
    Ok(Action::requeue(Duration::from_secs(30)))
}

async fn update_status(
    env: &EphemeralEnvironment,
    ctx: &Context,
    namespace: String,
    stack: Option<StackStatus>,
) -> Result<(), kube::Error> {
    let status = EphemeralEnvironmentStatus { namespace, stack };
    if env.status.as_ref() != Some(&status) {
        ctx.environments
            .patch_status(
                &env.name_any(),
                &PatchParams::default(),
                &Patch::Merge(json!({
                    "metadata": { "resourceVersion": env.resource_version() },
                    "status": status,
                })),
            )
            .await?;
    }
    Ok(())
}

async fn cleanup(env: &EphemeralEnvironment, ctx: &Context) -> Result<Action, ReconcileError> {
    let uid = environment_uid(env)?;
    let name = namespace_name(uid);
    let Some(ns) = ctx.namespaces.get_opt(&name).await? else {
        return Ok(Action::await_change());
    };

    if !owns_namespace(&ns, uid) {
        tracing::warn!(namespace = name, "leaving namespace owned by someone else");
        return Ok(Action::await_change());
    }
    if ns.metadata.deletion_timestamp.is_none() {
        if env.spec.api_postgres.is_some() {
            stack::uninstall(ctx.client.clone(), &name, CONTROL_NAMESPACE).await?;
        }
        let params = DeleteParams {
            preconditions: Some(Preconditions {
                uid: Some(
                    ns.uid()
                        .ok_or_else(|| ReconcileError::MissingUid(name.clone()))?,
                ),
                resource_version: ns.metadata.resource_version,
            }),
            ..DeleteParams::default()
        };
        ctx.namespaces.delete(&name, &params).await?;
    }

    // A DELETE acceptance isn't proof that the namespace is gone. Keep the
    // finalizer until a later GET returns NotFound.
    Err(ReconcileError::NamespaceDeleting)
}

fn error_policy(
    env: Arc<EphemeralEnvironment>,
    error: &finalizer::Error<ReconcileError>,
    _ctx: Arc<Context>,
) -> Action {
    if matches!(
        error,
        finalizer::Error::CleanupFailed(ReconcileError::NamespaceDeleting)
    ) {
        return Action::requeue(Duration::from_secs(2));
    }

    tracing::warn!(environment = env.name_any(), %error, "reconciliation failed");
    Action::requeue(Duration::from_secs(5))
}

fn environment_uid(env: &EphemeralEnvironment) -> Result<&str, ReconcileError> {
    env.metadata
        .uid
        .as_deref()
        .ok_or_else(|| ReconcileError::MissingUid(env.name_any()))
}

fn namespace_name(environment_uid: &str) -> String {
    format!("evanescent-{environment_uid}")
}

fn owns_namespace(ns: &Namespace, environment_uid: &str) -> bool {
    ns.labels().get(OWNER_LABEL).map(String::as_str) == Some(environment_uid)
}
