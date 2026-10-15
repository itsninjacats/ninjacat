#!/bin/bash
# The agent's data security check (Sensitive Data Scanner) against a Postgres with planted values.
L=$(dirname "$(readlink -f "$0")")
KEY=${LAB_KEY:-1ab00000000000000000000000000001}
NET=ninjacat-lab
docker rm -f dbmlab-db dbmlab-agent >/dev/null 2>&1
docker run -d --name dbmlab-db --network $NET -m 600m -e POSTGRES_PASSWORD=lab -v $L/sds/init:/docker-entrypoint-initdb.d:ro postgres:18-alpine >/dev/null
for i in $(seq 1 40); do docker exec dbmlab-db psql -U postgres -d shop -tc "SELECT count(*) FROM users" 2>/dev/null | grep -q 3 && break; sleep 2; done
docker run -d --name dbmlab-agent --network $NET -m 1g \
  -v $L/sds/conf:/etc/datadog-agent/conf.d/datasecurity.d:ro \
  -e DD_API_KEY=$KEY -e DD_HOSTNAME=dbmlab-sds -e DD_LOG_LEVEL=info \
  -e DD_DD_URL=http://app.ninjacat.lab:8080 -e DD_APM_ENABLED=false -e DD_PROCESS_AGENT_ENABLED=false \
  -e DD_SHARED_LIBRARY_CHECK_ENABLED=true -e DD_DATA_SECURITY_ENABLED=true \
  -e DD_SDS_RESULT_FORWARDER_LOGS_DD_URL=sds-intake.ninjacat.lab:8080 -e DD_SDS_RESULT_FORWARDER_LOGS_NO_SSL=true \
  -e DD_REMOTE_CONFIGURATION_ENABLED=false datadog/agent:7 >/dev/null
sleep 75
docker exec dbmlab-agent agent status 2>/dev/null | grep -B2 -A12 -i "datasecurity" | head -30
docker logs dbmlab-agent 2>&1 | grep -iE "shared librar|datasecurity|sds" | cut -c1-260 | sort | uniq -c | sort -rn | head -12
docker rm -f dbmlab-agent dbmlab-db >/dev/null
echo "sds: done"
