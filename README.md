# Evanescent

A Kubernetes operator for ephemeral environments.

Evanescent watches `EphemeralEnvironment` requests in `evanescent-system`. It
creates one Kubernetes namespace per request, records its name in
`.status.namespace`, and waits for that namespace to disappear when the request
is deleted. Requests can opt into a bundled PostgREST and Postgres demo stack
that the operator installs and upgrades with Helm v4.3.2, and set a lifetime
for automatic cleanup.

## Install a release on a cluster

Use a release that has a matching `ghcr.io/caelurium/evanescent` image, and
apply the manifests from that same release. These commands require cluster
administrator access to install the CRD, RBAC, and validating admission policy.

```sh
version=vX.Y.Z
git clone --depth 1 --branch "$version" https://github.com/caelurium/evanescent.git
cd evanescent
kubectl apply -f manifests/ephemeralenvironments.crd.yaml -f manifests/rbac.yaml
kubectl wait --for=condition=Established --timeout=60s crd/ephemeralenvironments.evanescent.caelurium.github.io
kubectl set image --local -f manifests/deployment.yaml \
  "evanescent=ghcr.io/caelurium/evanescent:$version" -o yaml | kubectl apply -f -
kubectl -n evanescent-system rollout status deployment/evanescent --timeout=90s
```

GHCR packages start private. A maintainer can make the package public for
anonymous pulls; otherwise configure an image pull secret in
`evanescent-system` and reference it from the `evanescent` ServiceAccount or
Deployment before deploying.

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
an API upgrade that preserves a database row, and automatic cleanup of the release,
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

### Set an environment lifetime

Set `spec.ttlSeconds` to expire a namespace-only or Helm environment. The
lifetime is measured from the request's `.metadata.creationTimestamp`, not
from readiness or the last update. Omit it for manual cleanup only. An
existing lifetime can be shortened, extended, or removed before deletion
starts. The accepted range is one second to 365 days.

```sh
kubectl apply -f examples/expiring-environment.yaml
kubectl -n evanescent-system get ephemeralenvironment short-lived -o yaml
```

The example lives for 15 seconds. `.status.expiresAt` shows the scheduled
deadline. At that point the operator deletes the request, which triggers the
same cleanup as a manual delete. Controller downtime or an in-flight Helm
operation can delay cleanup but does not reset the deadline.

### Tear down the test cluster

Keep the controller running until the request is deleted, then run
`kind delete cluster --name evanescent --kubeconfig ./kind.kubeconfig`.
Run only one controller instance; leader election is not implemented. Delete
all requests before uninstalling the controller so their namespaces can be
cleaned up.

## Releases

After checks pass on `main`, Release Please uses Conventional Commits to
update a release PR. Merging that PR publishes a GitHub release and tag, which
triggers a separate workflow to publish the matching `linux/amd64` and
`linux/arm64` operator image to GHCR. The image uses the GitHub release tag (for
example, `v0.4.0`), not `latest`. Nothing is published to crates.io.
If the registry publish fails, rerun the **Operator image** workflow for that
release.

To enable publishing, configure the `RELEASE_PLEASE_TOKEN` Actions secret
with access to create release PRs and publish releases in this repository.

### Use an approved OCI chart

A cluster administrator publishes the chart and creates a ConfigMap named
`evanescent-chart-catalog` in `evanescent-system`. Put a JSON object in the
`catalog.json` key. Each key in that object is a `spec.templateRef` a request
can select. For example, save the following as `catalog.json` and replace the
chart reference with the digest printed by `helm push`.

```json
{
  "web": {
    "chart": "oci://registry.example.com/team/charts/web@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "imageRepositories": {"image": "docker.io/library/nginx"},
    "replicaLimits": {"replicaCount": 3}
  }
}
```

```sh
kubectl -n evanescent-system create configmap evanescent-chart-catalog \
  --from-file=catalog.json --dry-run=client -o yaml | kubectl apply -f -
kubectl -n evanescent-system rollout restart deployment/evanescent
kubectl apply -f examples/chart-environment.yaml
```

Publish charts to an OCI registry reachable by the operator. Package any
dependencies inside the chart. Chart pulls use HTTPS by default. An
administrator can set `plainHttp: true` for a local development registry.
The packaged operator supports anonymous chart pulls by default. For a private
registry, mount a Helm registry credentials file and set `HELM_REGISTRY_CONFIG`
on the operator Deployment. Control access to the catalog and Deployment as
you would other operator configuration.

Approve only trusted charts and image repositories. Review chart defaults,
workload permissions, hooks, and value paths before approving them. Images
supplied by callers must match an approved repository and include a SHA-256
digest. Replica overrides must be between zero and the approved maximum. Map
keys are Helm value paths such as `app.image` and `app.replicaCount`. Charts
run in the allocated namespace under the operator's Helm RBAC and support
namespaced workloads such as Deployments, StatefulSets, Jobs, and Services.
Cluster-scoped resources and chart-installed CRDs are outside that permission
set.

After a request sets `templateRef`, it keeps that reference for its lifetime.
To roll out a new chart version to all requests using the reference, change
its catalog digest and restart the operator. To roll out a new app build to one
request, update its image digest. The same Helm release is upgraded in each
case. Status reports the generation that last passed Helm's release wait,
including Job completion, and the release revision. Use Kubernetes workload
status for ongoing health. Removing a catalog entry blocks new deployments
and upgrades for that reference. Deletion and TTL still uninstall its release.
