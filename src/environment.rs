//! The `EphemeralEnvironment` API.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A request for a dedicated Kubernetes namespace.
#[derive(CustomResource, Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[kube(
    group = "evanescent.caelurium.github.io",
    version = "v1alpha1",
    kind = "EphemeralEnvironment",
    plural = "ephemeralenvironments",
    namespaced,
    status = "EphemeralEnvironmentStatus"
)]
pub struct EphemeralEnvironmentSpec {}

/// The namespace allocated to this environment.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EphemeralEnvironmentStatus {
    /// Name of the workload namespace, not a workload readiness signal.
    pub namespace: String,
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
