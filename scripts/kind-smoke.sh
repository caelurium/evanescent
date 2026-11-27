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
namespace=
export KUBECONFIG="$workdir/kubeconfig"

env_kubectl() {
  kubectl -n "$system_namespace" "$@"
}

cleanup() {
  local status=$?
  if [[ -n "$proxy_pid" ]]; then
    kill "$proxy_pid" 2>/dev/null || :
    wait "$proxy_pid" 2>/dev/null || :
  fi
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
  rm -f "$workdir/kubeconfig" "$workdir/proxy.log"
  rmdir "$workdir"
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

# Hold the chart Deployment, then the namespace, to check cleanup ordering.
kubectl -n "$namespace" patch deployment api --type=merge \
  -p '{"metadata":{"finalizers":["evanescent.caelurium.github.io/smoke-hold"]}}'
kubectl patch namespace "$namespace" --type=merge \
  -p '{"metadata":{"finalizers":["evanescent.caelurium.github.io/smoke-hold"]}}'
env_kubectl delete "$environment" --wait=false
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
