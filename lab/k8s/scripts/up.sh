#!/usr/bin/env bash
# Brings up the whole end-to-end lab: a kind cluster running NinjaCat with a
# real Datadog agent stack shipping real telemetry into it.
#
# SAFETY: every kubectl and helm call below passes --context explicitly. The
# machine this was written on has a live remote production cluster as its
# default context, and a single unqualified `kubectl apply` would have gone
# there. Do not "simplify" this by relying on the current context.
set -euo pipefail

CLUSTER=ninjacat-lab
CTX="kind-${CLUSTER}"
NS=ninjacat-lab
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
CERTS="$HERE/.certs"
TRAEFIK_CHART=41.6.1

say() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
k()   { kubectl --context "$CTX" "$@"; }

# --- 1. cluster ------------------------------------------------------------
if kind get clusters 2>/dev/null | grep -qx "$CLUSTER"; then
  say "cluster $CLUSTER already exists, reusing"
else
  say "creating kind cluster $CLUSTER"
  kind create cluster --config "$HERE/kind.yaml"
fi
k cluster-info >/dev/null

# --- 2. certificates -------------------------------------------------------
say "lab CA and wildcard certificate"
"$HERE/scripts/certs.sh" "$CERTS"

# --- 3. images -------------------------------------------------------------
# Built on the host and side-loaded. The manifests use imagePullPolicy
# IfNotPresent so the node never tries a registry that has never heard of us.
say "building images"
docker build -q -t ninjacat/api:lab      "$ROOT/api"      | sed 's/^/  api:      /'
docker build -q -t ninjacat/frontend:lab "$ROOT/frontend" | sed 's/^/  frontend: /'

say "loading images into the cluster"
kind load docker-image --name "$CLUSTER" ninjacat/api:lab ninjacat/frontend:lab

# --- 4. namespace, secrets -------------------------------------------------
say "namespace and secrets"
k apply -f "$HERE/manifests/00-namespace.yaml" -f "$HERE/manifests/01-no-way-out.yaml"

# The chart's Ingresses name these two Secrets by default; both hold the lab's
# wildcard certificate.
k -n "$NS" delete secret ninjacat-intake-tls ninjacat-frontend-tls ninjacat-lab-ca --ignore-not-found >/dev/null
k -n "$NS" create secret tls ninjacat-intake-tls   --cert="$CERTS/tls.crt" --key="$CERTS/tls.key" >/dev/null
k -n "$NS" create secret tls ninjacat-frontend-tls --cert="$CERTS/tls.crt" --key="$CERTS/tls.key" >/dev/null
# The CA the Datadog agent will be told to trust via SSL_CERT_FILE.
k -n "$NS" create secret generic ninjacat-lab-ca \
    --from-file=ca.crt="$CERTS/ca.crt" >/dev/null

# --- 5. data stores --------------------------------------------------------
say "data stores"
k apply -f "$HERE/manifests/10-clickhouse.yaml" -f "$HERE/manifests/11-postgres.yaml"
k -n "$NS" rollout status deployment/clickhouse --timeout=300s
k -n "$NS" rollout status deployment/postgres   --timeout=180s

# --- 6. Traefik, then NinjaCat from its chart --------------------------------
# Traefik first: the chart looks for its CRDs to decide whether the redirect
# from HTTP is its to add. No LoadBalancer in kind, and none needed: the only
# clients are the agent's pods.
say "traefik"
helm repo add traefik https://traefik.github.io/charts >/dev/null 2>&1 || true
helm repo update traefik >/dev/null
helm --kube-context "$CTX" upgrade --install traefik traefik/traefik \
  --version "$TRAEFIK_CHART" \
  --namespace traefik --create-namespace \
  --set service.type=ClusterIP \
  --wait --timeout 4m

# The chart's hook Jobs migrate both schemas before the pods start, which is
# why the data stores have to be up first. A lab run is therefore also a test
# of the chart: hooks, Ingress, NetworkPolicy and all.
say "ninjacat (helm/ninjacat): migrations, intake, query, panel"
helm --kube-context "$CTX" upgrade --install ninjacat "$ROOT/helm/ninjacat" \
  --namespace "$NS" \
  -f "$HERE/ninjacat-values.yaml" \
  --set-string secrets.betterAuthSecret="$(openssl rand -hex 32)" \
  --wait --timeout 6m
k -n "$NS" rollout status deployment/ninjacat-intake   --timeout=240s
k -n "$NS" rollout status deployment/ninjacat-query    --timeout=240s
k -n "$NS" rollout status deployment/ninjacat-frontend --timeout=240s

# --- 7. the API key -----------------------------------------------------------
# The panel would do the same: store the hash, then NOTIFY, so the running
# intake re-reads its keys now instead of at its next 30-second refresh.
say "API key for the agent"
API_KEY="$(openssl rand -hex 16)"
HASH="$(printf '%s' "$API_KEY" | sha256sum | cut -d' ' -f1)"
k -n "$NS" exec deploy/postgres -- psql -U root -d local -q -c \
  "INSERT INTO api_key (name, tenant_id, key_hash, prefix)
   VALUES ('datadog-lab','default','$HASH','${API_KEY:0:8}')
   ON CONFLICT (key_hash) DO NOTHING;
   NOTIFY ninjacat_api_keys;" >/dev/null
echo "  key created (prefix ${API_KEY:0:8})"

say "host port for the panel"
k apply -f "$HERE/manifests/20-nodeports.yaml"

# --- 8. DNS ----------------------------------------------------------------
say "pointing *.ninjacat.lab at Traefik"
"$HERE/scripts/coredns-patch.sh" "$CTX"

# --- 9. the Datadog agent --------------------------------------------------
say "installing the Datadog agent stack"
helm repo add datadog https://helm.datadoghq.com >/dev/null 2>&1 || true
helm repo update datadog >/dev/null
helm --kube-context "$CTX" upgrade --install datadog datadog/datadog \
  --namespace "$NS" \
  -f "$HERE/datadog-values.yaml" \
  --set datadog.apiKey="$API_KEY" \
  --wait --timeout 8m

say "lab is up"
cat <<INFO

  kubectl --context $CTX -n $NS get pods

  panel        http://localhost:8888
  query API    kubectl --context $CTX -n $NS port-forward deploy/ninjacat-query 18081:8081
  ClickHouse   curl 'http://localhost:18123/?user=ninjacat&password=ninjacat' -d 'SELECT 1'

  see what arrived:  $HERE/scripts/verify.sh
  tear down:         $HERE/scripts/down.sh

  The agent needs a minute or two before the first orchestrator and container
  payloads appear; metrics and logs start sooner.
INFO
