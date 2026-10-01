#!/bin/bash
# The lab's server: the shipping image against a scratch ClickHouse database,
# on a network with no route out, under every Datadog hostname an agent is
# pointed at here. Needs the dev stack's databases (docker compose up -d postgres clickhouse migrate).
#
#   lab/server.sh up      build, start, mint a temporary API key
#   lab/server.sh down    remove everything it made, the scratch database and the key included
set -eu
ROOT=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
KEY=${LAB_KEY:-1ab00000000000000000000000000001}
NET=ninjacat-lab
ch() { docker exec ninjacat-clickhouse clickhouse-client -u ninjacat --password ninjacat -q "$1"; }
pg() { docker exec ninjacat-postgres psql -U root -d local -qc "$1"; }

case ${1:-} in
  up)
    docker build -q -t ninjacat/api:lab "$ROOT/api" >/dev/null
    ch "CREATE DATABASE IF NOT EXISTS ninjacat_lab"
    pg "INSERT INTO api_key (name, key_hash, prefix, tenant_id) VALUES ('lab (temporary)', '$(printf %s "$KEY" | sha256sum | cut -d' ' -f1)', 'lab', 'lab') ON CONFLICT DO NOTHING"
    docker network create --internal $NET >/dev/null
    mkdir -p "$ROOT/lab/captures" && chmod 777 "$ROOT/lab/captures"
    docker run -d --name ninjacat-lab-api --network ninjacat_default -v "$ROOT/lab/captures:/data/captures" \
      -e DEBUG=true -e NINJACAT_CAPTURE_DIR=/data/captures \
      -e DATABASE_URL=postgres://root:mysecretpassword@postgres:5432/local \
      -e CLICKHOUSE_HTTP_ADDR=clickhouse:8123 -e CLICKHOUSE_DB=ninjacat_lab \
      -e NINJACAT_SELFMON_TENANT=lab-self -e NINJACAT_LOGS_TCP_ADDR=off ninjacat/api:lab >/dev/null
    aliases=""
    for host in app agent-http-intake.logs process trace.agent dbm-metrics-intake sds-intake ndm-intake snmp-traps-intake ndmflow-intake netpath-intake; do
      aliases="$aliases --alias $host.ninjacat.lab"
    done
    docker network connect $aliases $NET ninjacat-lab-api
    echo "lab server up; rows land in ClickHouse database ninjacat_lab, tenant lab"
    ;;
  down)
    docker rm -f ninjacat-lab-api dbmlab-db dbmlab-agent netlab-agent netlab-snmp netlab-pg netlab-redis netlab-client-pg netlab-client-redis >/dev/null 2>&1 || true
    docker network rm $NET >/dev/null 2>&1 || true
    ch "DROP DATABASE IF EXISTS ninjacat_lab"
    pg "DELETE FROM api_key WHERE name = 'lab (temporary)' AND tenant_id = 'lab'"
    echo "lab server down"
    ;;
  *) echo "usage: $0 up|down"; exit 2 ;;
esac
