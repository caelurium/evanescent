use std::{io, process::Output, time::Duration};

use serde::Deserialize;
use thiserror::Error;
use tokio::{process::Command, time::timeout};

const RELEASE: &str = "evanescent";
const SOURCE_LABEL: &str = "evanescent.caelurium.github.io/source=oci";
const RELEASE_FILTER: &str = "^evanescent$";
const HELM_TIMEOUT: &str = "120s";
const INSPECT_TIMEOUT: Duration = Duration::from_secs(20);
// A failed upgrade can use the wait timeout again for its rollback.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(270);

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot run helm {operation}: {source}")]
    Process {
        operation: &'static str,
        source: io::Error,
    },
    #[error("helm {0} exceeded its process timeout")]
    Timeout(&'static str),
    #[error("helm {operation} exited with {status}")]
    Exit {
        operation: &'static str,
        status: std::process::ExitStatus,
    },
    #[error("could not decode the Helm release status")]
    InvalidStatus,
    #[error("the Helm release has an incomplete operation")]
    Pending,
    #[error("an existing Helm release is outside the chart catalog")]
    UnmanagedRelease,
}

#[derive(Deserialize)]
struct Release {
    revision: i32,
    status: String,
    #[serde(default)]
    description: String,
}

enum ReleaseAction {
    Keep(i32),
    Upgrade,
    Wait,
}

fn release_action(release: Option<Release>, desired: &str) -> ReleaseAction {
    match release {
        Some(release) if release.status == "deployed" && release.description == desired => {
            ReleaseAction::Keep(release.revision)
        }
        Some(release)
            if release.status.starts_with("pending-") || release.status == "uninstalling" =>
        {
            ReleaseAction::Wait
        }
        _ => ReleaseAction::Upgrade,
    }
}

pub async fn deploy(
    namespace: &str,
    chart: &str,
    desired: &str,
    extra_args: &[&str],
) -> Result<i32, Error> {
    let existing = latest_release(namespace).await?;
    let has_release = existing.is_some();
    match release_action(existing, desired) {
        ReleaseAction::Keep(revision) => return Ok(revision),
        // Don't delete data or guess whether an interrupted operation is safe to repeat.
        ReleaseAction::Wait => return Err(Error::Pending),
        ReleaseAction::Upgrade => {}
    }

    if has_release && !is_catalog_release(namespace).await? {
        return Err(Error::UnmanagedRelease);
    }

    let mut args = vec![
        RELEASE,
        "--install",
        chart,
        "--description",
        desired,
        "--hide-notes",
        "--skip-crds",
        "--wait=legacy",
        "--timeout",
        HELM_TIMEOUT,
        "--rollback-on-failure",
        "--history-max=5",
        "--reset-values",
        "--wait-for-jobs",
        "--labels",
        SOURCE_LABEL,
    ];
    args.extend_from_slice(extra_args);
    let output = run("upgrade", namespace, &args, RELEASE_TIMEOUT).await?;
    succeeded("upgrade", output)?;
    match release_action(latest_release(namespace).await?, desired) {
        ReleaseAction::Keep(revision) => Ok(revision),
        _ => Err(Error::InvalidStatus),
    }
}

pub async fn uninstall(namespace: &str) -> Result<(), Error> {
    let output = run(
        "uninstall",
        namespace,
        &[
            RELEASE,
            "--ignore-not-found",
            "--cascade=foreground",
            "--wait=legacy",
            "--timeout",
            HELM_TIMEOUT,
        ],
        RELEASE_TIMEOUT,
    )
    .await?;
    succeeded("uninstall", output)?;
    Ok(())
}

async fn latest_release(namespace: &str) -> Result<Option<Release>, Error> {
    let output = run(
        "history",
        namespace,
        &[RELEASE, "--output=json", "--max=1"],
        INSPECT_TIMEOUT,
    )
    .await?;
    if !output.status.success()
        && String::from_utf8_lossy(&output.stderr)
            .lines()
            .any(|line| line.trim() == "Error: release: not found")
    {
        return Ok(None);
    }
    decode_latest(&succeeded("history", output)?)
}

async fn is_catalog_release(namespace: &str) -> Result<bool, Error> {
    let output = run(
        "list",
        namespace,
        &[
            "--filter",
            RELEASE_FILTER,
            "--selector",
            SOURCE_LABEL,
            "--output=json",
        ],
        INSPECT_TIMEOUT,
    )
    .await?;
    let releases: Vec<serde::de::IgnoredAny> =
        serde_json::from_slice(&succeeded("list", output)?).map_err(|_| Error::InvalidStatus)?;
    Ok(!releases.is_empty())
}

fn succeeded(operation: &'static str, output: Output) -> Result<Vec<u8>, Error> {
    if output.status.success() {
        Ok(output.stdout)
    } else {
        // Helm output can contain rendered Secrets. Do not attach it to errors.
        Err(Error::Exit {
            operation,
            status: output.status,
        })
    }
}

fn decode_latest(stdout: &[u8]) -> Result<Option<Release>, Error> {
    let releases: Vec<Release> =
        serde_json::from_slice(stdout).map_err(|_| Error::InvalidStatus)?;
    Ok(releases.into_iter().max_by_key(|release| release.revision))
}

async fn run(
    operation: &'static str,
    namespace: &str,
    args: &[&str],
    deadline: Duration,
) -> Result<Output, Error> {
    let mut command = Command::new("helm");
    command
        .args([operation, "--namespace", namespace])
        .args(args)
        .env("HELM_DRIVER", "secret")
        .kill_on_drop(true);
    timeout(deadline, command.output())
        .await
        .map_err(|_| Error::Timeout(operation))?
        .map_err(|source| Error::Process { operation, source })
}

#[cfg(test)]
mod tests {
    use super::{ReleaseAction, decode_latest, release_action};

    #[test]
    fn only_mutates_releases_that_need_reconciliation() {
        let release = || {
            decode_latest(br#"[{"revision":3,"status":"deployed","description":"desired"}]"#)
                .unwrap()
        };
        assert!(matches!(
            release_action(release(), "desired"),
            ReleaseAction::Keep(3)
        ));
        assert!(matches!(
            release_action(release(), "changed"),
            ReleaseAction::Upgrade
        ));
        assert!(matches!(
            release_action(None, "desired"),
            ReleaseAction::Upgrade
        ));
    }

    #[test]
    fn leaves_pending_releases_for_explicit_recovery() {
        let release = decode_latest(br#"[{"revision":2,"status":"pending-upgrade"}]"#).unwrap();
        assert!(matches!(
            release_action(release, "desired"),
            ReleaseAction::Wait
        ));
    }
}
