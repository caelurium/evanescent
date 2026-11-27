# Evanescent

A Kubernetes operator for ephemeral environments.

Evanescent watches `EphemeralEnvironment` requests in `evanescent-system`. It
creates one Kubernetes namespace per request, records its name in
`.status.namespace`, and waits for that namespace to disappear when the request
is deleted. Requests can opt into a bundled PostgREST and Postgres demo stack
that the operator installs and upgrades with Helm v4.3.2. Environments do not
expire automatically.

## Development

```sh
cargo test --locked
```

Pull requests run formatting, Clippy, tests, documentation, and the shared
[Rust structure](https://github.com/caelurium/rust-structure) check.

### Try it with kind

With Docker running and `kind` 0.33 or later and `kubectl` installed:

```sh
bash scripts/kind-smoke.sh
```

This builds the operator image, runs it as the controller service account in a
fresh cluster, and checks Helm install, restart without replacing the release,
an API upgrade that preserves a database row, and cleanup of the release,
namespace, and persistent volume. It removes the test cluster on exit. GitHub
Actions runs the same test on pull requests.

To try it interactively, build and load the image, then deploy the controller
into a disposable cluster:

```sh
docker build -t evanescent:dev .
kind create cluster --name evanescent --kubeconfig ./kind.kubeconfig
export KUBECONFIG="$PWD/kind.kubeconfig"
kind load docker-image evanescent:dev --name evanescent
kubectl apply -f manifests/ephemeralenvironments.crd.yaml -f manifests/rbac.yaml
kubectl wait --for=condition=Established crd/ephemeralenvironments.evanescent.caelurium.github.io
kubectl apply -f manifests/deployment.yaml
kubectl -n evanescent-system rollout status deployment/evanescent --timeout=90s
```

Then, in another terminal:

```sh
export KUBECONFIG="$PWD/kind.kubeconfig"
kubectl apply -f examples/ephemeral-environment.yaml
kubectl -n evanescent-system get ephemeralenvironment demo -o yaml
kubectl -n evanescent-system delete ephemeralenvironment demo
```

To deploy the demo stack instead, apply `examples/api-postgres-environment.yaml`.
The cluster needs a default StorageClass with dynamic provisioning. Use
`.status.stack.ready` and check that `.status.stack.observedGeneration` matches
`.metadata.generation` before treating the stack as ready. Changing
`spec.apiPostgres.apiVersion` from `v16.5` to `v16.6` upgrades the same release
without replacing the database. Delete the request to remove the stack.

The demo is for trusted, disposable clusters. It exposes an unauthenticated
API via an internal Service and initializes a synthetic `notes` table. It does
not accept arbitrary images, external charts, or user-supplied credentials.
Postgres credentials are generated per environment. Helm stores release data
in namespace-local Secrets. The controller can bind its Helm role only in
its workload namespaces, enforced by a validating admission policy.

If an interrupted Helm command leaves a pending release, the controller
reports `ReleasePending` rather than forcing an upgrade or deleting the
database. An administrator can inspect the release, recover it, or delete
the environment to tear it down.

Keep the controller running until the request is deleted, then run
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
