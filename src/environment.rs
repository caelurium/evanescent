//! The `EphemeralEnvironment` API.

use std::collections::BTreeMap;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A request for a dedicated Kubernetes namespace and optional Helm release.
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
    "rule": "!has(oldSelf.templateRef) || (has(self.templateRef) && self.templateRef == oldSelf.templateRef)",
    "message": "templateRef is immutable once set"
}, {
    "rule": "!has(self.values) || has(self.templateRef)",
    "message": "values requires templateRef"
}]))]
pub struct EphemeralEnvironmentSpec {
    /// Approved catalog entry. Once set, this reference stays with the environment.
    #[schemars(
        length(min = 1, max = 63),
        regex(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$")
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_ref: Option<String>,
    /// Overrides explicitly permitted by the catalog entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<ChartValues>,
    /// Lifetime in seconds from creation. Omit to disable automatic deletion.
    #[schemars(range(min = 1, max = 31_536_000))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<i32>,
}

/// Digest image and replica count overrides, keyed by Helm values paths.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct ChartValues {
    /// Image references pinned to SHA-256 digests.
    #[serde(default)]
    pub images: BTreeMap<String, String>,
    /// Replica counts bounded by the administrator's template rules.
    #[serde(default)]
    pub replicas: BTreeMap<String, i32>,
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
    /// The template or its requested values are absent from the catalog.
    TemplateRejected,
    /// A release from outside the chart catalog is awaiting cleanup.
    UnmanagedRelease,
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
