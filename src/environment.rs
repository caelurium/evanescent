//! The `EphemeralEnvironment` API.

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A request for a dedicated Kubernetes namespace and optional demo stack.
#[derive(CustomResource, Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[kube(
    group = "evanescent.caelurium.github.io",
    version = "v1alpha1",
    kind = "EphemeralEnvironment",
    plural = "ephemeralenvironments",
    namespaced,
    status = "EphemeralEnvironmentStatus"
)]
#[serde(rename_all = "camelCase")]
#[schemars(extend("x-kubernetes-validations" = [{
    "rule": "!has(oldSelf.apiPostgres) || has(self.apiPostgres)",
    "message": "delete the environment to remove its API and Postgres stack"
}]))]
pub struct EphemeralEnvironmentSpec {
    /// Opt in to the bundled PostgREST and Postgres chart.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_postgres: Option<ApiPostgresSpec>,
    /// Lifetime in seconds from creation. Omit to disable automatic deletion.
    #[schemars(range(min = 1, max = 31_536_000))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<i32>,
}

/// The settings users can change on the bundled chart.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiPostgresSpec {
    /// An approved PostgREST version. Changing it upgrades the same release.
    pub api_version: PostgrestVersion,
}

/// Supported PostgREST versions for the demo stack.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
pub enum PostgrestVersion {
    /// PostgREST 16.5.
    #[serde(rename = "v16.5")]
    V16_5,
    /// PostgREST 16.6.
    #[serde(rename = "v16.6")]
    V16_6,
}

/// The namespace allocated to this environment.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EphemeralEnvironmentStatus {
    /// Name of the workload namespace, not a workload readiness signal.
    pub namespace: String,
    /// Scheduled time for automatic deletion, when a lifetime is set.
    pub expires_at: Option<Time>,
    /// Readiness of the requested stack, if one was requested.
    pub stack: Option<StackStatus>,
}

/// The result of reconciling the chart and checking its workloads.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackStatus {
    /// Spec generation that was checked. Older generations may be stale.
    pub observed_generation: i64,
    /// Whether the requested release and its workloads are ready.
    pub ready: bool,
    /// The result of the check.
    pub reason: StackReason,
    /// Revision of the release, when known.
    pub release_revision: Option<i32>,
}

/// Outcomes of a stack reconciliation.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub enum StackReason {
    /// The desired release is deployed and its workloads are available.
    ReleaseReady,
    /// The release is deployed but one or more workloads are unavailable.
    WorkloadsNotReady,
    /// The Kubernetes API or Helm returned an error.
    ReconcileFailed,
    /// Helm recorded an incomplete operation.
    ReleasePending,
}

#[cfg(test)]
mod tests {
    use kube::CustomResourceExt;
    use serde_json::Value;

    use super::EphemeralEnvironment;

    #[test]
    fn manifest_matches_the_api_type() {
        let manifest: Value =
            serde_saphyr::from_str(include_str!("../manifests/ephemeralenvironments.crd.yaml"))
                .unwrap();
        let expected = serde_json::to_value(EphemeralEnvironment::crd()).unwrap();

        assert_eq!(manifest, expected);
    }
}
