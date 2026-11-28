use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    LimitRange, LimitRangeItem, LimitRangeSpec, ResourceQuota, ResourceQuotaSpec,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::{
    Api, Client,
    api::{ObjectMeta, PostParams},
};

const NAME: &str = "evanescent";

/// Bootstrap defaults for containers and a namespace-wide resource budget.
pub async fn ensure(client: Client, namespace: &str) -> Result<(), kube::Error> {
    let limits: Api<LimitRange> = Api::namespaced(client.clone(), namespace);
    if limits.get_opt(NAME).await?.is_none() {
        limits
            .create(
                &PostParams::default(),
                &LimitRange {
                    metadata: metadata(),
                    spec: Some(LimitRangeSpec {
                        limits: vec![LimitRangeItem {
                            type_: "Container".into(),
                            default: Some(quantities([("cpu", "250m"), ("memory", "256Mi")])),
                            default_request: Some(quantities([
                                ("cpu", "100m"),
                                ("memory", "64Mi"),
                            ])),
                            ..LimitRangeItem::default()
                        }],
                    }),
                },
            )
            .await?;
    }
    let quotas: Api<ResourceQuota> = Api::namespaced(client, namespace);
    if quotas.get_opt(NAME).await?.is_none() {
        quotas
            .create(
                &PostParams::default(),
                &ResourceQuota {
                    metadata: metadata(),
                    spec: Some(ResourceQuotaSpec {
                        hard: Some(quantities([
                            ("pods", "20"),
                            ("requests.cpu", "4"),
                            ("requests.memory", "8Gi"),
                            ("limits.cpu", "8"),
                            ("limits.memory", "16Gi"),
                        ])),
                        ..ResourceQuotaSpec::default()
                    }),
                    ..ResourceQuota::default()
                },
            )
            .await?;
    }
    Ok(())
}

fn metadata() -> ObjectMeta {
    ObjectMeta {
        name: Some(NAME.into()),
        ..ObjectMeta::default()
    }
}

fn quantities<const N: usize>(values: [(&str, &str); N]) -> BTreeMap<String, Quantity> {
    values
        .into_iter()
        .map(|(key, value)| (key.to_owned(), Quantity(value.to_owned())))
        .collect()
}
