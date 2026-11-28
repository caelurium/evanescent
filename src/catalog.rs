use std::{collections::BTreeMap, env};

use evanescent::environment::ChartValues;
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

const CATALOG_ENV: &str = "EVANESCENT_CHART_CATALOG";

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot read {CATALOG_ENV}")]
    Environment(#[source] env::VarError),
    #[error("invalid JSON in {CATALOG_ENV}")]
    Json(#[source] serde_json::Error),
    #[error("a chart catalog entry has an invalid reference or value rule")]
    InvalidEntry,
    #[error("templateRef is absent from the chart catalog")]
    UnknownTemplate,
    #[error("a requested Helm value is not approved for this template")]
    UnapprovedValue,
}

#[derive(Default, Deserialize)]
#[serde(transparent)]
pub struct Catalog(BTreeMap<String, Template>);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Template {
    chart: String,
    #[serde(default)]
    plain_http: bool,
    #[serde(default)]
    image_repositories: BTreeMap<String, String>,
    #[serde(default)]
    replica_limits: BTreeMap<String, i32>,
}

pub struct Release<'a> {
    pub chart: &'a str,
    pub plain_http: bool,
    pub overrides: Vec<String>,
    pub description: String,
}

impl Catalog {
    pub fn from_env() -> Result<Self, Error> {
        match env::var(CATALOG_ENV) {
            Ok(source) => Self::parse(&source),
            Err(env::VarError::NotPresent) => Ok(Self::default()),
            Err(error) => Err(Error::Environment(error)),
        }
    }

    fn parse(source: &str) -> Result<Self, Error> {
        let catalog: Self = serde_json::from_str(source).map_err(Error::Json)?;
        for template in catalog.0.values() {
            let keys = template
                .image_repositories
                .keys()
                .chain(template.replica_limits.keys())
                .collect::<Vec<_>>();
            if !valid_oci_chart(&template.chart)
                || keys.iter().any(|key| !valid_key(key))
                || template.replica_limits.values().any(|limit| *limit < 0)
                || keys.iter().enumerate().any(|(index, key)| {
                    keys[index + 1..].iter().any(|other| {
                        key == other
                            || key.starts_with(&format!("{other}."))
                            || other.starts_with(&format!("{key}."))
                    })
                })
            {
                return Err(Error::InvalidEntry);
            }
        }
        Ok(catalog)
    }

    pub fn resolve(&self, name: &str, values: Option<&ChartValues>) -> Result<Release<'_>, Error> {
        let template = self.0.get(name).ok_or(Error::UnknownTemplate)?;
        let mut overrides = Vec::new();
        if let Some(values) = values {
            for (key, image) in &values.images {
                let (repository, digest) = image.split_once('@').ok_or(Error::UnapprovedValue)?;
                if !valid_digest(digest)
                    || !template
                        .image_repositories
                        .get(key)
                        .is_some_and(|approved| approved == repository)
                {
                    return Err(Error::UnapprovedValue);
                }
                overrides.push(format!("{key}={}", json!(image)));
            }
            for (key, count) in &values.replicas {
                if !template
                    .replica_limits
                    .get(key)
                    .is_some_and(|max| (0..=*max).contains(count))
                {
                    return Err(Error::UnapprovedValue);
                }
                overrides.push(format!("{key}={count}"));
            }
        }
        let description = format!("evanescent/oci/{}", json!([template.chart, overrides]));
        Ok(Release {
            chart: &template.chart,
            plain_http: template.plain_http,
            overrides,
            description,
        })
    }
}

fn valid_key(key: &str) -> bool {
    key.split('.').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    })
}

fn valid_oci_chart(chart: &str) -> bool {
    chart
        .strip_prefix("oci://")
        .and_then(|reference| reference.split_once('@'))
        .is_some_and(|(path, digest)| {
            path.split_once('/').is_some_and(|(host, repository)| {
                !host.is_empty()
                    && !repository.is_empty()
                    && host
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
                    && repository.bytes().all(|b| {
                        b.is_ascii_lowercase()
                            || b.is_ascii_digit()
                            || matches!(b, b'.' | b'/' | b'-' | b'_')
                    })
            }) && valid_digest(digest)
        })
}

fn valid_digest(digest: &str) -> bool {
    digest.strip_prefix("sha256:").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn catalog_requires_a_digest_and_nonoverlapping_paths() {
        assert!(Catalog::parse(r#"{"demo":{"chart":"oci://example.com/charts/demo"}}"#).is_err());
        let source = json!({"demo": {
            "chart": format!("oci://example.com/charts/demo@{DIGEST}"),
            "imageRepositories": {"app.image": "example.com/images/demo"},
            "replicaLimits": {"app": 2},
        }});
        assert!(Catalog::parse(&source.to_string()).is_err());
    }

    #[test]
    fn request_can_only_use_approved_digest_images_and_counts() {
        let catalog = Catalog::parse(
            &json!({"demo": {
                "chart": format!("oci://example.com/charts/demo@{DIGEST}"),
                "imageRepositories": {"app.image": "example.com/images/demo"},
                "replicaLimits": {"app.replicas": 2},
            }})
            .to_string(),
        )
        .unwrap();
        let image = format!("example.com/images/demo@{DIGEST}");
        let mut values = ChartValues {
            images: BTreeMap::from([("app.image".into(), image.clone())]),
            replicas: BTreeMap::from([("app.replicas".into(), 2)]),
        };
        let release = catalog.resolve("demo", Some(&values)).unwrap();
        assert_eq!(
            release.overrides,
            [
                format!("app.image=\"{image}\""),
                "app.replicas=2".to_string()
            ]
        );
        assert_ne!(
            release.description,
            catalog.resolve("demo", None).unwrap().description
        );
        assert!(catalog.resolve("missing", None).is_err());
        values.replicas.insert("app.replicas".into(), 3);
        assert!(catalog.resolve("demo", Some(&values)).is_err());
        values.replicas.clear();
        values
            .images
            .insert("app.image".into(), "example.com/images/demo:latest".into());
        assert!(catalog.resolve("demo", Some(&values)).is_err());
        values.images.insert(
            "app.image".into(),
            format!("other.example.com/demo@{DIGEST}"),
        );
        assert!(catalog.resolve("demo", Some(&values)).is_err());
    }
}
