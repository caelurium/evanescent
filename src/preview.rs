//! Defaults for ephemeral preview environments.

use std::time::Duration;

/// Default lifetime of a preview environment: one day.
///
/// ```
/// use evanescent::preview::DEFAULT_TTL;
///
/// assert_eq!(DEFAULT_TTL.as_secs(), 86_400);
/// ```
pub const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);
