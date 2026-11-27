# Evanescent

A Rust Kubernetes operator for ephemeral environments.

## Layout and ownership

- `src/environment.rs` defines the `EphemeralEnvironment` API. Keep the Rust
  types, `manifests/ephemeralenvironments.crd.yaml`, and
  `examples/ephemeral-environment.yaml` in sync.
- `src/controller.rs` owns reconciliation and namespace cleanup.
  `src/main.rs` handles startup and client setup.
- `manifests/rbac.yaml` defines the controller's permissions.
  `manifests/deployment.yaml` runs the image built by `Dockerfile`.
  `scripts/kind-smoke.sh` exercises the lifecycle against a real Kubernetes API.

## Rust modules

- Keep modules focused on one capability. Keep visibility narrow and document
  public behavior and guarantees.
- Aim for **800 non-test lines per Rust file**; **1,500 is the hard cap**.
  Keep test files and inline test modules under **1,000 lines**.
- Split capability families into directories. A folder over 40 Rust source
  files warns and over 60 fails. See `.github/rust-structure.json` and the
  shared [checker](https://github.com/caelurium/rust-structure).
- Keep module declarations and re-exports sorted, one item per line.
- Use comments for non-obvious constraints and rationale, not to restate code.

## Reconciliation and security

- Make reconciliation safe to repeat after retries and restarts. Keep
  Kubernetes calls asynchronous and use requeues rather than blocking workers.
- Add the finalizer before creating workload resources. Use the request UID to
  identify its namespace, verify ownership before deleting, and keep the
  finalizer until the owned namespace is gone.
- Keep request watches scoped to `evanescent-system` and grant only the RBAC
  permissions the controller needs. A dedicated namespace is not a security
  boundary for untrusted workloads.
- Treat `.status.namespace` as the name of the allocated namespace, not a
  signal that workloads are ready.
- Preserve useful Kubernetes error context without logging credentials or
  secret values.

## Commands and verification

Run the relevant CI checks locally:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps
```

For changes to reconciliation, RBAC, manifests, or the operator image, run
`bash scripts/kind-smoke.sh` with Docker, kind, and kubectl. The PR workflow also
runs the smoke test and the shared Rust structure check.

## Contributions

- Keep changes focused, preserve unrelated work, and use Conventional Commits.
- Open a PR for completed work and follow `.github/PULL_REQUEST_TEMPLATE.md`.
  Report validation results in the handoff, not the PR body.
- Use repository-relative paths and synthetic examples. Do not commit
  kubeconfigs, tokens, real workload data, or machine-specific diagnostics.
