#!/bin/bash
# The lab's server: every intake program of the server's image, with DEBUG on, against a scratch
# ClickHouse database, behind the dev stack's routing (dev/intake-routes.yml), on a network with
# no route out, under every Datadog hostname an agent is pointed at here. Needs the dev stack's
# databases (docker compose up -d postgres clickhouse migrate).
#
#   lab/server.sh up      build, migrate the scratch database, start, mint a temporary API key
#   lab/server.sh down    remove everything it made, the scratch database and the key included
set -eu
ROOT=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
KEY=${LAB_KEY:-1ab00000000000000000000000000001}
NET=ninjacat-lab
# Between the router and the programs: their names there are the ones the routes file uses, and
# no other network has the router on it, so the dev stack's programs are never reached.
BACK=ninjacat-lab-back
IMAGE=ninjapaws:lab
# One per program in backend/api/src/bin, as the routes file names them.
PROGRAMS=$(sed -n 's|.*url: http://intake-\(.*\):8080|\1|p' "$ROOT/dev/intake-routes.yml")
ch() { docker exec ninjacat-clickhouse clickhouse-client -u ninjacat --password ninjacat -q "$1"; }
pg() { docker exec ninjacat-postgres psql -U root -d local -qc "$1"; }
server() {
  docker run --network ninjacat_default --user "$(id -u):$(id -g)" \
    -e DATABASE_URL=postgres://root:mysecretpassword@postgres:5432/local \
    -e CLICKHOUSE_HTTP_ADDR=clickhouse:8123 -e CLICKHOUSE_DB=ninjacat_lab "$@"
}

case ${1:-} in
  up)
    docker build -q --build-arg PROFILE=dev -t $IMAGE "$ROOT/backend" >/dev/null
    ch "CREATE DATABASE IF NOT EXISTS ninjacat_lab"
    server --rm $IMAGE ninjapaws_migrate >/dev/null
    pg "INSERT INTO api_key (name, key_hash, prefix, tenant_id) VALUES ('lab (temporary)', '$(printf %s "$KEY" | sha256sum | cut -d' ' -f1)', 'lab', 'lab') ON CONFLICT DO NOTHING"
    docker network create --internal $NET >/dev/null
    docker network create --internal $BACK >/dev/null
    mkdir -p "$ROOT/lab/captures"
    for program in $PROGRAMS; do
      server -d --name ninjacat-lab-$program -v "$ROOT/lab/captures:/captures" \
        -e DEBUG=true -e NINJACAT_CAPTURE_DIR=/captures -e NINJACAT_LOGS_TCP_ADDR=off \
        $IMAGE "ninjapaws_${program//-/_}" >/dev/null
      docker network connect --alias intake-$program $BACK ninjacat-lab-$program
    done
    docker run -d --name ninjacat-lab-intake --network $BACK -v "$ROOT/dev/intake-routes.yml:/etc/traefik/intake-routes.yml:ro" \
      traefik:v3.5.3 --entrypoints.intake.address=:8080 --providers.file.filename=/etc/traefik/intake-routes.yml >/dev/null
    aliases=""
    for host in app agent-http-intake.logs process trace.agent dbm-metrics-intake sds-intake ndm-intake snmp-traps-intake ndmflow-intake netpath-intake; do
      aliases="$aliases --alias $host.ninjacat.lab"
    done
    docker network connect $aliases $NET ninjacat-lab-intake
    echo "lab server up; rows land in ClickHouse database ninjacat_lab, tenant lab; requests in lab/captures"
    ;;
  down)
    docker rm -f ninjacat-lab-intake $(for program in $PROGRAMS; do echo ninjacat-lab-$program; done) \
      dbmlab-db dbmlab-agent netlab-agent netlab-snmp netlab-pg netlab-redis netlab-client-pg netlab-client-redis >/dev/null 2>&1 || true
    docker network rm $NET $BACK >/dev/null 2>&1 || true
    ch "DROP DATABASE IF EXISTS ninjacat_lab"
    pg "DELETE FROM api_key WHERE name = 'lab (temporary)' AND tenant_id = 'lab'"
    echo "lab server down"
    ;;
  *) echo "usage: $0 up|down"; exit 2 ;;
esac
