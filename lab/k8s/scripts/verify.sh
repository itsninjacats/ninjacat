#!/usr/bin/env bash
#
# Proves the lab is actually working end to end: the Datadog agent collected
# real telemetry, NinjaCat ingested it, and the rows landed in ClickHouse.
#
# All queries go THROUGH kubectl exec into the ClickHouse pod, so this works
# even when the host port mapping is broken — one less variable when
# debugging.
#
# Usage: verify.sh [kube-context]     (default: kind-ninjacat-lab)
set -euo pipefail

CTX="${1:-kind-ninjacat-lab}"
NS=ninjacat-lab

# Find the ClickHouse pod. No pod means the stack is not up (or the wrong
# context was passed) — fail loudly rather than exec into nothing.
POD=$(kubectl --context "$CTX" -n "$NS" get pod -l app=clickhouse \
        -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true)
if [[ -z "$POD" ]]; then
    echo "ERROR: no clickhouse pod found in namespace $NS (context $CTX)." >&2
    echo "Is the lab up? Did you pass the right context?" >&2
    exit 1
fi

# Everything below authenticates the same way the applications do; a passing
# query also proves the ninjacat user and database exist.
ch() {
    kubectl --context "$CTX" -n "$NS" exec "$POD" -- \
        clickhouse-client -u ninjacat --password ninjacat \
        --database ninjacat -q "$1"
}

echo "== Row counts per table =="
# Non-zero counts across the board prove every intake path delivered:
# metrics/logs/hosts/processes/check_runs/events come from the core agent,
# the k8s_* tables from the cluster agent's orchestrator collection, and
# container_* from the container lifecycle/image intakes.
ch "
SELECT table, rows FROM (
          SELECT 'metrics'               AS table, count() AS rows FROM metrics
UNION ALL SELECT 'logs',                  count() FROM logs
UNION ALL SELECT 'hosts',                 count() FROM hosts
UNION ALL SELECT 'processes',             count() FROM processes
UNION ALL SELECT 'check_runs',            count() FROM check_runs
UNION ALL SELECT 'events',                count() FROM events
UNION ALL SELECT 'k8s_resources',         count() FROM k8s_resources
UNION ALL SELECT 'k8s_resources_current', count() FROM k8s_resources_current
UNION ALL SELECT 'k8s_manifests',         count() FROM k8s_manifests
UNION ALL SELECT 'k8s_cluster',           count() FROM k8s_cluster
UNION ALL SELECT 'container_events',      count() FROM container_events
UNION ALL SELECT 'container_images',      count() FROM container_images
) ORDER BY table
FORMAT PrettyCompact"

echo
echo "== Kubernetes and system metric names (sample) =="
# kubernetes.* only exists if kubelet/orchestrator checks ran INSIDE a real
# cluster; system.* proves node-level collection. Together they show these
# are agent-collected series, not synthetic inserts.
ch "
SELECT DISTINCT metric FROM metrics
WHERE metric LIKE 'kubernetes.%' OR metric LIKE 'system.%'
ORDER BY metric LIMIT 25
FORMAT PrettyCompact"

echo
echo "== Newest log lines =="
# Fresh timestamps with a service tag prove the log pipeline is live and
# tagging works, not just that some rows exist from an old run.
ch "
SELECT timestamp, service, substring(message, 1, 120) AS message
FROM logs ORDER BY timestamp DESC LIMIT 5
FORMAT PrettyCompact"

echo
echo "== Resource kinds in k8s_resources =="
# Multiple kinds (Pod, Deployment, Node, ...) prove the cluster agent's
# orchestrator collection is walking the whole API, not a single resource.
ch "
SELECT kind, count() AS rows FROM k8s_resources
GROUP BY kind ORDER BY rows DESC
FORMAT PrettyCompact"

echo
echo "Verification queries completed."
