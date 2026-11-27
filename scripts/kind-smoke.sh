#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
workdir="$(mktemp -d)"
cluster="evanescent-smoke-$$"
image="evanescent:$cluster"
system_namespace=evanescent-system
manifest=examples/ephemeral-environment.yaml
environment=ephemeralenvironment/demo
deployment=deployment/evanescent
selector=app.kubernetes.io/name=evanescent
export KUBECONFIG="$workdir/kubeconfig"

env_kubectl() {
  kubectl -n "$system_namespace" "$@"
}

cleanup() {
  local status=$?
  if (( status != 0 )); then
    env_kubectl --request-timeout=5s describe pods -l "$selector" || :
    env_kubectl --request-timeout=5s logs "$deployment" --tail=100 || :
    env_kubectl --request-timeout=5s get ephemeralenvironments -o yaml || :
    kubectl --request-timeout=5s get namespaces || :
    kubectl --request-timeout=5s get events -A || :
  fi
  kind delete cluster --name "$cluster" || :
  docker image rm "$image" 2>/dev/null || :
  rm -f "$workdir/kubeconfig"
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

kubectl apply -f "$manifest"
request_uid="$(env_kubectl get "$environment" -o jsonpath='{.metadata.uid}')"
namespace="evanescent-$request_uid"
env_kubectl wait --for="jsonpath={.status.namespace}=$namespace" --timeout=60s "$environment"
namespace_uid="$(kubectl get namespace "$namespace" -o jsonpath='{.metadata.uid}')"

# Clear status while the controller is down so only the new pod can restore it.
env_kubectl scale "$deployment" --replicas=0
env_kubectl wait --for=delete --timeout=90s pod -l "$selector"
env_kubectl patch "$environment" --subresource=status --type=merge -p '{"status":null}'
env_kubectl scale "$deployment" --replicas=1
env_kubectl rollout status "$deployment" --timeout=90s
env_kubectl wait --for="jsonpath={.status.namespace}=$namespace" --timeout=60s "$environment"
[[ "$(kubectl get namespace "$namespace" -o jsonpath='{.metadata.uid}')" == "$namespace_uid" ]]

# Hold the namespace in Terminating to prove the request waits for cleanup.
kubectl patch namespace "$namespace" --type=merge \
  -p '{"metadata":{"finalizers":["evanescent.caelurium.github.io/smoke-hold"]}}'
env_kubectl delete "$environment" --wait=false
kubectl wait --for='jsonpath={.status.phase}=Terminating' --timeout=60s "namespace/$namespace"
env_kubectl get "$environment" -o jsonpath='{.metadata.finalizers[*]}' |
  grep -Fx 'evanescent.caelurium.github.io/namespace-cleanup'
kubectl patch namespace "$namespace" --type=merge -p '{"metadata":{"finalizers":null}}'
env_kubectl wait --for=delete --timeout=90s "$environment"
remaining="$(kubectl get namespace "$namespace" --ignore-not-found -o name)"
[[ -z "$remaining" ]]
