#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
workdir="$(mktemp -d)"
cluster="evanescent-smoke-$$"
image="evanescent:$cluster"
system_namespace=evanescent-system
manifest=examples/api-postgres-environment.yaml
environment=ephemeralenvironment/demo-stack
deployment=deployment/evanescent
selector=app.kubernetes.io/name=evanescent
proxy_pid=
forward_pid=
namespace=
export KUBECONFIG="$workdir/kubeconfig"

env_kubectl() {
  kubectl -n "$system_namespace" "$@"
}

stop_forward() {
  if [[ -n "$forward_pid" ]]; then
    kill "$forward_pid" 2>/dev/null || :
    wait "$forward_pid" 2>/dev/null || :
    forward_pid=
  fi
}

port_forward() {
  local target_port=$1
  shift
  kubectl port-forward "$@" ":$target_port" > "$workdir/forward.log" 2>&1 &
  forward_pid=$!
  forwarded_port=
  for ((attempt = 0; attempt < 50; attempt++)); do
    forwarded_port="$(sed -n "s/^Forwarding from 127[.]0[.]0[.]1:\([0-9]*\) -> [0-9]*$/\1/p" "$workdir/forward.log")"
    [[ -z "$forwarded_port" ]] || return 0
    kill -0 "$forward_pid" 2>/dev/null || break
    sleep 0.1
  done
  cat "$workdir/forward.log" >&2
  return 1
}

cleanup() {
  local status=$?
  if [[ -n "$proxy_pid" ]]; then
    kill "$proxy_pid" 2>/dev/null || :
    wait "$proxy_pid" 2>/dev/null || :
  fi
  stop_forward
  if (( status != 0 )); then
    env_kubectl --request-timeout=5s describe pods -l "$selector" || :
    env_kubectl --request-timeout=5s logs "$deployment" --tail=100 || :
    env_kubectl --request-timeout=5s get ephemeralenvironments -o yaml || :
    if [[ -n "$namespace" ]]; then
      kubectl -n "$namespace" --request-timeout=5s describe pods || :
      kubectl -n "$namespace" --request-timeout=5s get pvc || :
    fi
    kubectl --request-timeout=5s get namespaces || :
    kubectl --request-timeout=5s get events -A || :
  fi
  kind delete cluster --name "$cluster" || :
  docker image rm "$image" 2>/dev/null || :
  rm -rf "$workdir"
}
trap cleanup EXIT

docker build --tag "$image" .
kind create cluster --name "$cluster" --kubeconfig "$KUBECONFIG" --wait 120s \
  --image kindest/node:v1.37.0@sha256:a1ed56cfb0e7b93589bdf97c8cd566405a265939e3620fc4f5de89adff580ae5
kind load docker-image "$image" --name "$cluster"
kubectl apply -f manifests/ephemeralenvironments.crd.yaml -f manifests/rbac.yaml
kubectl wait --for=condition=Established --timeout=60s crd/ephemeralenvironments.evanescent.caelurium.github.io
kubectl set image --local -f manifests/deployment.yaml "evanescent=$image" -o yaml |
  kubectl apply -f -
env_kubectl rollout status "$deployment" --timeout=90s

helm_version="$(env_kubectl exec "$deployment" -- helm version --template '{{.Version}}')"
if [[ "${helm_version%%+*}" != v4.3.2 ]]; then
  printf 'Expected bundled Helm v4.3.2, got %s\n' "$helm_version" >&2
  exit 1
fi
printf 'Bundled Helm: %s\n' "$helm_version"

# Cluster-wide RoleBinding creation must not grant Helm permissions elsewhere.
if denial="$(env_kubectl create rolebinding evanescent-helm --dry-run=server \
  --as=system:serviceaccount:evanescent-system:evanescent \
  --clusterrole=evanescent-helm --serviceaccount=evanescent-system:evanescent 2>&1)"; then
  printf 'Unexpectedly granted Helm access in the control namespace\n' >&2
  exit 1
fi
grep -F 'Evanescent can bind Helm access only inside its workload namespaces.' <<<"$denial"

kubectl apply -f "$manifest"
request_uid="$(env_kubectl get "$environment" -o jsonpath='{.metadata.uid}')"
namespace="evanescent-$request_uid"
env_kubectl wait --for="jsonpath={.status.namespace}=$namespace" --timeout=60s "$environment"
namespace_uid="$(kubectl get namespace "$namespace" -o jsonpath='{.metadata.uid}')"
env_kubectl wait --for='jsonpath={.status.stack.ready}=true' --timeout=180s "$environment"
[[ "$(env_kubectl get "$environment" -o jsonpath='{.status.stack.releaseRevision}')" == 1 ]]
if denial="$(env_kubectl patch "$environment" --dry-run=server --type=merge -p '{"spec":{"apiPostgres":null}}' 2>&1)"; then
  printf 'Unexpectedly removed the stack from an existing environment\n' >&2
  exit 1
fi
grep -F 'delete the environment to remove its API and Postgres stack' <<<"$denial"

# Clear status while the controller is down so only the new pod can restore it.
env_kubectl scale "$deployment" --replicas=0
env_kubectl wait --for=delete --timeout=90s pod -l "$selector"
env_kubectl patch "$environment" --subresource=status --type=merge -p '{"status":null}'
env_kubectl scale "$deployment" --replicas=1
env_kubectl rollout status "$deployment" --timeout=90s
env_kubectl wait --for="jsonpath={.status.namespace}=$namespace" --timeout=60s "$environment"
[[ "$(kubectl get namespace "$namespace" -o jsonpath='{.metadata.uid}')" == "$namespace_uid" ]]
env_kubectl wait --for='jsonpath={.status.stack.ready}=true' --timeout=60s "$environment"
[[ "$(env_kubectl get "$environment" -o jsonpath='{.status.stack.releaseRevision}')" == 1 ]]

# Use Kubernetes' service proxy on a random loopback port so no ingress is needed.
kubectl proxy --port=0 > "$workdir/proxy.log" 2>&1 &
proxy_pid=$!
port=
for ((attempt = 0; attempt < 50; attempt++)); do
  port="$(sed -n 's/^Starting to serve on 127\.0\.0\.1:\([0-9][0-9]*\)$/\1/p' "$workdir/proxy.log")"
  [[ -z "$port" ]] || break
  kill -0 "$proxy_pid"
  sleep 0.1
done
[[ -n "$port" ]] || { cat "$workdir/proxy.log" >&2; exit 1; }
notes="http://127.0.0.1:$port/api/v1/namespaces/$namespace/services/http:api:3000/proxy/notes"
curl --fail-with-body --silent --show-error --max-time 15 \
  --header 'Content-Type: application/json' --data '{"id":42,"body":"survived upgrade"}' "$notes"
volume="$(kubectl -n "$namespace" get pvc data-postgres-0 -o jsonpath='{.spec.volumeName}')"
[[ -n "$volume" ]]

env_kubectl patch "$environment" --type=merge -p '{"spec":{"apiPostgres":{"apiVersion":"v16.6"}}}'
env_kubectl wait --for='jsonpath={.status.stack.releaseRevision}=2' --timeout=180s "$environment"
env_kubectl wait --for='jsonpath={.status.stack.ready}=true' --timeout=60s "$environment"
generation="$(env_kubectl get "$environment" -o jsonpath='{.metadata.generation}')"
[[ "$(env_kubectl get "$environment" -o jsonpath='{.status.stack.observedGeneration}')" == "$generation" ]]
[[ "$(kubectl -n "$namespace" get deployment api -o jsonpath='{.spec.template.spec.containers[0].image}')" == postgrest/postgrest:v16.6@sha256:* ]]
[[ "$(kubectl -n "$namespace" get pvc data-postgres-0 -o jsonpath='{.spec.volumeName}')" == "$volume" ]]
curl --fail-with-body --silent --show-error --max-time 15 "$notes?id=eq.42" |
  grep -F 'survived upgrade'

# Shorten the lifetime of the Helm environment and check that it cleans up in order.
# Hold the chart Deployment first, then the namespace.
kubectl -n "$namespace" patch deployment api --type=merge \
  -p '{"metadata":{"finalizers":["evanescent.caelurium.github.io/smoke-hold"]}}'
kubectl patch namespace "$namespace" --type=merge \
  -p '{"metadata":{"finalizers":["evanescent.caelurium.github.io/smoke-hold"]}}'
env_kubectl patch "$environment" --type=merge -p '{"spec":{"ttlSeconds":1}}'
kubectl -n "$namespace" wait --for='jsonpath={.metadata.deletionTimestamp}' --timeout=60s deployment/api
[[ "$(kubectl get namespace "$namespace" -o jsonpath='{.status.phase}')" == Active ]]
kubectl -n "$namespace" patch deployment api --type=merge -p '{"metadata":{"finalizers":null}}'
kubectl wait --for='jsonpath={.status.phase}=Terminating' --timeout=180s "namespace/$namespace"
env_kubectl get "$environment" -o jsonpath='{.metadata.finalizers[*]}' |
  grep -Fx 'evanescent.caelurium.github.io/namespace-cleanup'
release_secrets="$(kubectl -n "$namespace" get secrets -l owner=helm,name=evanescent -o name)"
[[ -z "$release_secrets" ]]
kubectl patch namespace "$namespace" --type=merge -p '{"metadata":{"finalizers":null}}'
env_kubectl wait --for=delete --timeout=90s "$environment"
remaining="$(kubectl get namespace "$namespace" --ignore-not-found -o name)"
[[ -z "$remaining" ]]
kubectl wait --for=delete --timeout=90s "pv/$volume"

# The original empty spec must still provision a namespace without Helm access.
kubectl apply -f examples/ephemeral-environment.yaml
env_kubectl wait --for='jsonpath={.status.namespace}' --timeout=60s ephemeralenvironment/demo
namespace="$(env_kubectl get ephemeralenvironment/demo -o jsonpath='{.status.namespace}')"
binding="$(kubectl -n "$namespace" get rolebinding evanescent-helm --ignore-not-found -o name)"
[[ -z "$binding" ]]
env_kubectl delete ephemeralenvironment/demo --timeout=90s

# An idle request with a future deadline should expire without another spec change.
kubectl apply -f examples/expiring-environment.yaml
environment=ephemeralenvironment/short-lived
request_uid="$(env_kubectl get "$environment" -o jsonpath='{.metadata.uid}')"
namespace="evanescent-$request_uid"
env_kubectl wait --for="jsonpath={.status.namespace}=$namespace" --timeout=60s "$environment"
env_kubectl wait --for='jsonpath={.status.expiresAt}' --timeout=60s "$environment"
kubectl get namespace "$namespace" -o name
env_kubectl wait --for=delete --timeout=90s "$environment"
remaining="$(kubectl get namespace "$namespace" --ignore-not-found -o name)"
[[ -z "$remaining" ]]

# An administrator publishes and approves a database-free chart by digest.
registry="deployment/smoke-registry"
env_kubectl create deployment smoke-registry --port=5000 \
  --image=registry@sha256:82f6ff4a7f982960c8aa51617fea3aabc485df6e55cce80d22deb1de36f1bc3c
env_kubectl expose "$registry" --port=5000
env_kubectl rollout status "$registry" --timeout=90s
port_forward 5000 -n "$system_namespace" service/smoke-registry

# Use the exact Helm client shipped in the image for the OCI round trip.
env_kubectl exec "$deployment" -- cat /usr/local/bin/helm > "$workdir/helm"
chmod +x "$workdir/helm"
"$workdir/helm" package scripts/fixtures/web --destination "$workdir"
push_output="$("$workdir/helm" push --plain-http "$workdir/web-0.2.0.tgz" \
  "oci://127.0.0.1:$forwarded_port/smoke" 2>&1)"
chart_digest="$(sed -n 's/^Digest: //p' <<<"$push_output")"
[[ "$chart_digest" =~ ^sha256:[0-9a-f]{64}$ ]] || { echo "$push_output" >&2; exit 1; }
stop_forward

# Use a real ingress controller and inspect HTTP through its Service.
curl --fail --silent --show-error --location --retry 3 \
  https://traefik.github.io/charts/traefik/traefik-43.0.0.tgz -o "$workdir/traefik.tgz"
echo "29cbf019c5e013d9455c89fa29be66d2dd9007c84ed51b969220225fd1b7983c  $workdir/traefik.tgz" | sha256sum --check
"$workdir/helm" install evanescent-preview "$workdir/traefik.tgz" \
  --namespace smoke-ingress --create-namespace --skip-crds \
  --set service.spec.type=ClusterIP,ingressClass.name=evanescent-preview,ingressClass.isDefaultClass=false \
  --set providers.kubernetesIngress.ingressClass=evanescent-preview,providers.kubernetesCRD.enabled=false \
  --set image.versionOverride=v3.8.0,image.digest=sha256:00c9734983e74ae35a1fc18eaee9e9da13f3dde95c4e45b3b8c3b60e0dbd2d17
kubectl -n smoke-ingress rollout status deployment/evanescent-preview-traefik --timeout=120s
port_forward 80 -n smoke-ingress service/evanescent-preview-traefik
preview="http://127.0.0.1:$forwarded_port"

cat > "$workdir/catalog.json" <<EOF
{
  "web": {
    "chart": "oci://smoke-registry.$system_namespace.svc:5000/smoke/web@$chart_digest",
    "plainHttp": true,
    "imageRepositories": {"image": "docker.io/library/nginx"},
    "replicaLimits": {"replicaCount": 2}
  }
}
EOF
env_kubectl create configmap evanescent-chart-catalog --from-file="$workdir/catalog.json"
env_kubectl rollout restart "$deployment"
env_kubectl rollout status "$deployment" --timeout=90s

kubectl apply -f examples/chart-environment.yaml
environment=ephemeralenvironment/demo-web
namespace="evanescent-$(env_kubectl get "$environment" -o jsonpath='{.metadata.uid}')"
env_kubectl wait --for='jsonpath={.status.stack.ready}=true' --timeout=180s "$environment"
[[ "$(env_kubectl get "$environment" -o jsonpath='{.status.stack.releaseRevision}')" == 1 ]]
kubectl -n "$namespace" rollout status deployment/web --timeout=90s
[[ "$(kubectl -n "$namespace" get deployment/web -o jsonpath='{.status.readyReplicas}')" == 2 ]]
uid="$(kubectl -n "$namespace" get deployment/web -o jsonpath='{.metadata.uid}')"

host="$namespace.preview.example.com"
curl --fail --silent --show-error --retry 15 --retry-all-errors --retry-delay 1 --max-time 5 \
  --header "Host: $host" "$preview/" | grep -F 'Welcome to nginx!'

if denial="$(kubectl -n "$namespace" patch ingress web --dry-run=server --type=merge \
  -p '{"spec":{"ingressClassName":"unapproved"}}' 2>&1)"; then
  printf 'Accepted an unapproved ingress class\n' >&2
  exit 1
fi
grep -F 'Use the approved preview IngressClass' <<<"$denial"
if denial="$(kubectl -n "$namespace" patch ingress web --dry-run=server --type=json \
  -p '[{"op":"replace","path":"/spec/rules/0/host","value":"other.preview.example.com"}]' 2>&1)"; then
  printf 'Accepted an ingress for another environment\n' >&2
  exit 1
fi
grep -F 'Ingress rules must use explicit hostnames' <<<"$denial"
if denial="$(kubectl -n "$namespace" annotate ingress web --dry-run=server \
  nginx.ingress.kubernetes.io/server-alias=other.preview.example.com 2>&1)"; then
  printf 'Accepted an unapproved routing annotation\n' >&2
  exit 1
fi
grep -F 'Ingress annotations must be approved' <<<"$denial"

kubectl -n "$namespace" wait --for='jsonpath={.status.hard.pods}=20' --timeout=30s resourcequota/evanescent
if denial="$(kubectl -n "$namespace" apply --dry-run=server -f - 2>&1 <<'EOF'
apiVersion: v1
kind: Pod
metadata:
  name: over-quota
spec:
  containers:
    - name: web
      image: docker.io/library/nginx@sha256:a8b39bd9cf0f83869a2162827a0caf6137ddf759d50a171451b335cecc87d236
      resources:
        requests: {cpu: "5", memory: 64Mi}
        limits: {cpu: "5", memory: 64Mi}
EOF
)"; then
  printf 'Accepted a workload over the namespace CPU quota\n' >&2
  exit 1
fi
grep -F 'exceeded quota: evanescent' <<<"$denial"

# A catalog violation changes status without replacing or upgrading the release.
env_kubectl patch "$environment" --type=merge -p '{"spec":{"values":{"replicas":{"replicaCount":3}}}}'
env_kubectl wait --for='jsonpath={.status.stack.reason}=TemplateRejected' --timeout=60s "$environment"
[[ "$(kubectl -n "$namespace" get deployment/web -o jsonpath='{.spec.replicas}')" == 2 ]]
updated_image=docker.io/library/nginx@sha256:5616878291a2eed594aee8db4dade5878cf7edcb475e59193904b198d9b830de
env_kubectl patch "$environment" --type=merge \
  -p "{\"spec\":{\"values\":{\"replicas\":{\"replicaCount\":2},\"images\":{\"image\":\"$updated_image\"}}}}"
env_kubectl wait --for='jsonpath={.status.stack.releaseRevision}=2' --timeout=180s "$environment"
kubectl -n "$namespace" rollout status deployment/web --timeout=90s
[[ "$(kubectl -n "$namespace" get deployment/web -o jsonpath='{.metadata.uid}')" == "$uid" ]]
[[ "$(kubectl -n "$namespace" get deployment/web -o jsonpath='{.spec.template.spec.containers[0].image}')" == "$updated_image" ]]
[[ "$(kubectl -n "$namespace" get deployment/web -o jsonpath='{.status.readyReplicas}')" == 2 ]]

# Cleanup uses the stored release even if the catalog entry is removed.
env_kubectl delete configmap evanescent-chart-catalog
env_kubectl rollout restart "$deployment"
env_kubectl rollout status "$deployment" --timeout=90s
kubectl patch namespace "$namespace" --type=merge \
  -p '{"metadata":{"finalizers":["evanescent.caelurium.github.io/smoke-hold"]}}'
env_kubectl patch "$environment" --type=merge -p '{"spec":{"ttlSeconds":1}}'
kubectl wait --for='jsonpath={.status.phase}=Terminating' --timeout=180s "namespace/$namespace"
[[ -z "$(kubectl -n "$namespace" get secrets -l owner=helm,name=evanescent -o name)" ]]
kubectl patch namespace "$namespace" --type=merge -p '{"metadata":{"finalizers":null}}'
env_kubectl wait --for=delete --timeout=90s "$environment"
[[ -z "$(kubectl get namespace "$namespace" --ignore-not-found -o name)" ]]

status=
for ((attempt = 0; attempt < 30; attempt++)); do
  status="$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' --max-time 3 \
    --header "Host: $host" "$preview/")"
  [[ "$status" != 404 ]] || break
  sleep 1
done
[[ "$status" == 404 ]]
