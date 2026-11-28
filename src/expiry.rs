use std::time::Duration;

use evanescent::environment::EphemeralEnvironment;
use k8s_openapi::{
    apimachinery::pkg::apis::meta::v1::Time,
    jiff::{self, SignedDuration, Timestamp},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("environment has no creation timestamp")]
    MissingCreationTimestamp,
    #[error("cannot calculate the expiry deadline: {0}")]
    InvalidDeadline(#[source] jiff::Error),
}

pub fn deadline(env: &EphemeralEnvironment) -> Result<Option<Time>, Error> {
    let Some(ttl) = env.spec.ttl_seconds else {
        return Ok(None);
    };
    let created_at = env
        .metadata
        .creation_timestamp
        .as_ref()
        .ok_or(Error::MissingCreationTimestamp)?;
    created_at
        .0
        .checked_add(SignedDuration::from_secs(i64::from(ttl)))
        .map(|stamp| Some(Time(stamp)))
        .map_err(Error::InvalidDeadline)
}

pub fn next_check(deadline: Option<&Time>, now: Timestamp, fallback: Duration) -> Duration {
    deadline.map_or(fallback, |time| {
        Duration::try_from(time.0.duration_since(now))
            .unwrap_or_default()
            .min(fallback)
    })
}

pub fn is_due(deadline: Option<&Time>, now: Timestamp) -> bool {
    deadline.is_some_and(|time| time.0 <= now)
}

#[cfg(test)]
mod tests {
    use evanescent::environment::EphemeralEnvironmentSpec;

    use super::{
        Duration, EphemeralEnvironment, Error, Time, Timestamp, deadline, is_due, next_check,
    };

    fn environment(ttl_seconds: Option<i32>) -> EphemeralEnvironment {
        EphemeralEnvironment::new(
            "demo",
            EphemeralEnvironmentSpec {
                api_postgres: None,
                template_ref: None,
                values: None,
                ttl_seconds,
            },
        )
    }

    fn timestamp(value: &str) -> Timestamp {
        value.parse().unwrap()
    }

    #[test]
    fn lifetime_is_opt_in() {
        assert!(deadline(&environment(None)).unwrap().is_none());
        assert!(matches!(
            deadline(&environment(Some(10))),
            Err(Error::MissingCreationTimestamp)
        ));
    }

    #[test]
    fn lifetime_edits_use_the_original_creation_time() {
        let mut env = environment(Some(15));
        env.metadata.creation_timestamp = Some(Time(timestamp("2026-11-27T22:00:00Z")));
        assert_eq!(
            deadline(&env).unwrap(),
            Some(Time(timestamp("2026-11-27T22:00:15Z")))
        );
        env.spec.ttl_seconds = Some(60);
        assert_eq!(
            deadline(&env).unwrap(),
            Some(Time(timestamp("2026-11-27T22:01:00Z")))
        );
        env.spec.ttl_seconds = None;
        assert!(deadline(&env).unwrap().is_none());
    }

    #[test]
    fn schedules_the_deadline_without_skipping_regular_checks() {
        let expires_at = Time(timestamp("2026-11-27T22:00:10Z"));
        let regular = Duration::from_secs(30);
        let early = timestamp("2026-11-27T21:59:00Z");
        let just_before = timestamp("2026-11-27T22:00:09.750Z");
        let after = timestamp("2026-11-27T22:00:11Z");
        assert!(!is_due(None, after));
        assert!(!is_due(Some(&expires_at), just_before));
        assert!(is_due(Some(&expires_at), expires_at.0));
        assert!(is_due(Some(&expires_at), after));
        assert_eq!(next_check(None, early, regular), regular);
        assert_eq!(next_check(Some(&expires_at), early, regular), regular);
        assert_eq!(
            next_check(Some(&expires_at), just_before, regular),
            Duration::from_millis(250)
        );
        assert_eq!(
            next_check(Some(&expires_at), expires_at.0, regular),
            Duration::ZERO
        );
        assert_eq!(
            next_check(Some(&expires_at), after, regular),
            Duration::ZERO
        );
    }
}
