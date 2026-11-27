#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
workdir="$(mktemp -d)"
cluster="evanescent-smoke-$$"
controller_pid=
system_namespace=evanescent-system
manifest=examples/ephemeral-environment.yaml
environment=ephemeralenvironment/demo
export KUBECONFIG="$workdir/kubeconfig"

stop_controller() {
  if [[ -n "$controller_pid" ]]; then
    kill "$controller_pid" 2>/dev/null || :
    wait "$controller_pid" || :
    controller_pid=
  fi
}

cleanup() {
  local status=$?
  stop_controller
  if (( status != 0 )); then
    cat "$workdir"/controller*.log 2>/dev/null || :
    kubectl --request-timeout=5s get ephemeralenvironments -n "$system_namespace" -o yaml || :
    kubectl --request-timeout=5s get namespaces || :
    kubectl --request-timeout=5s get events -A || :
  fi
  kind delete cluster --name "$cluster" || :
  rm -f "$workdir/kubeconfig" "$workdir/controller.kubeconfig" "$workdir"/controller*.log
  rmdir "$workdir"
}
trap cleanup EXIT

start_controller() {
  KUBECONFIG="$workdir/controller.kubeconfig" target/debug/evanescent > "$workdir/controller-$1.log" 2>&1 &
  controller_pid=$!
}

env_kubectl() {
  kubectl -n "$system_namespace" "$@"
}

cargo build --locked --target-dir target
kind create cluster --name "$cluster" --kubeconfig "$KUBECONFIG" --wait 120s \
  --image kindest/node:v1.37.0@sha256:a1ed56cfb0e7b93589bdf97c8cd566405a265939e3620fc4f5de89adff580ae5
kubectl apply -f manifests/ephemeralenvironments.crd.yaml -f manifests/rbac.yaml
kubectl wait --for=condition=Established --timeout=60s crd/ephemeralenvironments.evanescent.caelurium.github.io

# Run the controller as the service account rather than as the kind admin.
token="$(env_kubectl create token evanescent --duration=10m)"
kubectl config view --raw --flatten --minify -o json |
  jq --arg token "$token" '
    .users = [{name: "evanescent", user: {token: $token}}]
    | .contexts[0].context.user = "evanescent"
  ' > "$workdir/controller.kubeconfig"
unset token

start_controller initial
kubectl apply -f "$manifest"
request_uid="$(env_kubectl get "$environment" -o jsonpath='{.metadata.uid}')"
namespace="evanescent-$request_uid"
env_kubectl wait --for="jsonpath={.status.namespace}=$namespace" --timeout=60s "$environment"
namespace_uid="$(kubectl get namespace "$namespace" -o jsonpath='{.metadata.uid}')"

stop_controller
# Force an observable reconciliation after restart.
env_kubectl patch "$environment" --subresource=status --type=merge -p '{"status":null}'
start_controller restarted
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
