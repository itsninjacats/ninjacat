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
docker build -q -t ninjacat/server:lab   "$ROOT/server"   | sed 's/^/  server:   /'
docker build -q -t ninjacat/frontend:lab "$ROOT/frontend" | sed 's/^/  frontend: /'

say "loading images into the cluster"
kind load docker-image --name "$CLUSTER" ninjacat/server:lab ninjacat/frontend:lab

# --- 4. namespace, secrets, schema ----------------------------------------
say "namespace and secrets"
k apply -f "$HERE/manifests/00-namespace.yaml"

k -n "$NS" delete secret ninjacat-lab-tls ninjacat-lab-ca ninjacat-lab-frontend --ignore-not-found >/dev/null
k -n "$NS" create secret tls ninjacat-lab-tls \
    --cert="$CERTS/tls.crt" --key="$CERTS/tls.key" >/dev/null
# The CA the Datadog agent will be told to trust via SSL_CERT_FILE.
k -n "$NS" create secret generic ninjacat-lab-ca \
    --from-file=ca.crt="$CERTS/ca.crt" >/dev/null
k -n "$NS" create secret generic ninjacat-lab-frontend \
    --from-literal=betterAuthSecret="$(openssl rand -base64 32)" >/dev/null

# No ClickHouse schema ConfigMap any more: the server embeds its migrations
# (server/schema) and applies them at startup, so the schema still has exactly
# one home — the server binary the lab just built.

# --- 5. data stores --------------------------------------------------------
say "data stores"
k apply -f "$HERE/manifests/10-clickhouse.yaml" -f "$HERE/manifests/11-postgres.yaml"
k -n "$NS" rollout status deployment/clickhouse --timeout=300s
k -n "$NS" rollout status deployment/postgres   --timeout=180s

# --- 6. migrations, then the API key --------------------------------------
# Order matters: the Go server refuses to start without the api_key table, and
# its keeper reads the keys once at startup. Creating the key BEFORE the server
# exists means its first fetch already sees it — no refresh call needed.
say "database migrations"
k -n "$NS" delete job db-migrate --ignore-not-found >/dev/null
k apply -f "$HERE/manifests/22-migrate-job.yaml"
k -n "$NS" wait --for=condition=complete job/db-migrate --timeout=180s

say "API key for the agent"
API_KEY="$(openssl rand -hex 16)"
HASH="$(printf '%s' "$API_KEY" | sha256sum | cut -d' ' -f1)"
k -n "$NS" exec deploy/postgres -- psql -U root -d local -q -c \
  "INSERT INTO api_key (name, tenant_id, key_hash, prefix)
   VALUES ('datadog-lab','default','$HASH','${API_KEY:0:8}')
   ON CONFLICT (key_hash) DO NOTHING;" >/dev/null
echo "  key created (prefix ${API_KEY:0:8})"

# --- 7. application + TLS front door --------------------------------------
say "ninjacat, panel and TLS proxy"
k apply -f "$HERE/manifests/20-ninjacat.yaml" \
        -f "$HERE/manifests/21-frontend.yaml" \
        -f "$HERE/manifests/30-proxy.yaml"
k -n "$NS" rollout status deployment/ninjacat       --timeout=240s
k -n "$NS" rollout status deployment/ninjacat-proxy --timeout=120s
k -n "$NS" rollout status deployment/frontend       --timeout=240s

# --- 8. DNS ----------------------------------------------------------------
say "pointing *.ninjacat.lab at the proxy"
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
  panel API    http://localhost:18081/ping
  ClickHouse   curl 'http://localhost:18123/?user=ninjacat&password=ninjacat' -d 'SELECT 1'

  see what arrived:  $HERE/scripts/verify.sh
  tear down:         $HERE/scripts/down.sh

  The agent needs a minute or two before the first orchestrator and container
  payloads appear; metrics and logs start sooner.
INFO
