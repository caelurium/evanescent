# Evanescent

A Kubernetes operator for ephemeral environments.

Evanescent watches `EphemeralEnvironment` requests in `evanescent-system`. It
creates one Kubernetes namespace per request, records its name in
`.status.namespace`, and waits for that namespace to disappear when the request
is deleted. It does not install workloads or expire environments automatically
yet.

## Development

```sh
cargo test --locked
```

Pull requests run formatting, Clippy, tests, documentation, and the shared
[Rust structure](https://github.com/caelurium/rust-structure) check.

### Try it with kind

With Docker running and `kind` 0.33 or later, `kubectl`, and `jq` installed:

```sh
bash scripts/kind-smoke.sh
```

This builds the controller, creates a fresh local cluster, and checks namespace
creation, restart without replacing the namespace, and cleanup. It removes the
test cluster on exit. GitHub Actions runs the same test on pull requests.

To try it interactively, install the CRD and RBAC and start the controller
against a disposable cluster:

```sh
kind create cluster --name evanescent --kubeconfig ./kind.kubeconfig
export KUBECONFIG="$PWD/kind.kubeconfig"
kubectl apply -f manifests/ephemeralenvironments.crd.yaml -f manifests/rbac.yaml
kubectl wait --for=condition=Established crd/ephemeralenvironments.evanescent.caelurium.github.io
cargo run --locked
```

Then, in another terminal:

```sh
export KUBECONFIG="$PWD/kind.kubeconfig"
kubectl apply -f examples/ephemeral-environment.yaml
kubectl -n evanescent-system get ephemeralenvironment demo -o yaml
kubectl -n evanescent-system delete ephemeralenvironment demo
```

Keep the controller running until the request is deleted, then stop it and run
`kind delete cluster --name evanescent --kubeconfig ./kind.kubeconfig`.
Run only one controller instance; leader election is not implemented. Delete
all requests before uninstalling the controller so their namespaces can be
cleaned up.

## Releases

After checks pass on `main`, Release Please uses Conventional Commits to
update a release PR. Merging that PR publishes a GitHub release and tag; it
does not publish to crates.io.

To enable publishing, configure the `RELEASE_PLEASE_TOKEN` Actions secret
with access to create release PRs and publish releases in this repository.
