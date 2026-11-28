# Evanescent

A Kubernetes operator for ephemeral environments.

Evanescent watches `EphemeralEnvironment` requests in `evanescent-system`. It
creates one Kubernetes namespace per request, records its name in
`.status.namespace`, and waits for that namespace to disappear when the request
is deleted. Requests can select an approved OCI Helm chart and set a lifetime
for automatic cleanup. Evanescent installs, upgrades, and uninstalls each
release with its bundled Helm v4.3.2 CLI.

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
fresh cluster, and checks OCI chart installation, restart without replacing
the release, an in-place image upgrade, Ingress routing, quota enforcement,
and automatic release and namespace cleanup. It removes the test cluster on
exit. GitHub Actions runs the same test on pull requests.

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

### Upgrade from the bundled Postgres demo

The bundled PostgREST and Postgres chart and `spec.apiPostgres` are retired.
Before applying the new CRD or Deployment, find any demo environments with
the old CRD still installed.

```sh
kubectl -n evanescent-system get ephemeralenvironments \
  -o 'custom-columns=NAME:.metadata.name,POSTGREST:.spec.apiPostgres.apiVersion,NAMESPACE:.status.namespace'
```

Export any data you need from those environments. To run the old and new
stacks during migration, first install a transitional release that supports
both `apiPostgres` and `templateRef`. Publish and approve your own chart,
create a new request with `templateRef`, restore the data, and verify the new
environment. Delete the old request and wait for its namespace to disappear
before applying the retirement release. For a direct upgrade, export the data
and delete the old request before upgrading, then restore into a catalog
environment after the upgrade.

If you upgrade first, an old Helm release can remain in the workload namespace
until you delete or expire its request. Evanescent reports
`UnmanagedRelease` and retains the ability to uninstall it. Create a new
request to migrate to a catalog chart. Existing releases with a pending Helm
operation report `ReleasePending` when reconciling a chart. An administrator
can recover the release or delete its environment to tear it down.

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

## Use an approved OCI chart

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

### Configure preview routing and quotas

Charts can own standard `networking.k8s.io/v1` Ingress resources. The platform
administrator installs an ingress controller and sets the approved class and
domain in the `evanescent-ingresses` admission policy in
`manifests/rbac.yaml`. The defaults are `evanescent-preview` and
`preview.example.com`. Configure DNS and TLS for the chosen domain and
IngressClass on the cluster.

Set `spec.ingressClassName` in the chart. Each rule must use the allocated
namespace as a subdomain, for example
`{{ .Release.Namespace }}.preview.example.com` or
`api.{{ .Release.Namespace }}.preview.example.com`. Charts can choose paths,
backend Services, and TLS configuration for those hosts. The policy rejects
wildcards, hostless rules, and routes for other environments. Administrators
can add trusted controller-specific annotation keys to the policy's
`allowedAnnotations` list after reviewing how they affect routing. Helm updates
and deletes the Ingress with the release. DNS and HTTP availability are
independent of the Helm release readiness check.

Evanescent creates a ResourceQuota and LimitRange named `evanescent` before
deploying any chart. The default quota allows 20 pods, 4 requested CPUs,
8 GiB of requested memory, 8 CPUs in limits, and 16 GiB in memory limits.
Containers without resource settings get requests of 100 millicores and
64 MiB and limits of 250 millicores and 256 MiB. Administrators can edit those
objects in a workload namespace. Reconciliation preserves existing budgets.
When setting replica counts, reserve quota for pods created during rollouts.
