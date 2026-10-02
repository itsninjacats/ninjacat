#!/bin/bash
# system-probe (network monitoring, service monitoring, traceroute) on a privileged agent,
# with Postgres, Redis and HTTP traffic between lab containers for it to see.
N=$(dirname "$(readlink -f "$0")")
KEY=${LAB_KEY:-1ab00000000000000000000000000001}
NET=ninjacat-lab
docker rm -f netlab-agent netlab-pg netlab-redis netlab-client-pg netlab-client-redis >/dev/null 2>&1
docker run -d --name netlab-pg --network $NET -m 400m -e POSTGRES_PASSWORD=lab postgres:18-alpine >/dev/null
docker run -d --name netlab-redis --network $NET -m 200m redis:7-alpine >/dev/null
sleep 8
docker exec netlab-pg psql -U postgres -qc "CREATE TABLE orders (id serial PRIMARY KEY, customer text, amount numeric); INSERT INTO orders (customer, amount) SELECT 'c' || (i % 50), i FROM generate_series(1, 500) i;"
chmod -R a+rX $N
docker run -d --name netlab-agent --network $NET -m 2g --cgroupns host --pid host \
  -v /var/run/docker.sock:/var/run/docker.sock:ro -v /proc/:/host/proc/:ro -v /sys/fs/cgroup/:/host/sys/fs/cgroup:ro -v /sys/kernel/debug:/sys/kernel/debug \
  --security-opt apparmor:unconfined --cap-add=SYS_ADMIN --cap-add=SYS_RESOURCE --cap-add=SYS_PTRACE --cap-add=NET_ADMIN \
  --cap-add=NET_BROADCAST --cap-add=NET_RAW --cap-add=IPC_LOCK --cap-add=CHOWN \
  -v $N/datadog.yaml:/etc/datadog-agent/datadog.yaml:ro -v $N/conf/network_path.d/conf.yaml:/etc/datadog-agent/conf.d/network_path.d/conf.yaml:ro \
  -e DD_API_KEY=$KEY -e DD_HOSTNAME=netlab-probe \
  -e DD_PROCESS_AGENT_ENABLED=true -e DD_PROCESS_CONFIG_PROCESS_DD_URL=http://process.ninjacat.lab:8080 \
  -e DD_SYSTEM_PROBE_NETWORK_ENABLED=true -e DD_SYSTEM_PROBE_SERVICE_MONITORING_ENABLED=true -e DD_TRACEROUTE_ENABLED=true \
  -e DD_SERVICE_MONITORING_CONFIG_ENABLE_POSTGRES_MONITORING=true -e DD_SERVICE_MONITORING_CONFIG_POSTGRES_ENABLED=true \
  -e DD_SERVICE_MONITORING_CONFIG_ENABLE_REDIS_MONITORING=true -e DD_SERVICE_MONITORING_CONFIG_REDIS_ENABLED=true \
  -e DD_SERVICE_MONITORING_CONFIG_ENABLE_HTTP2_MONITORING=true -e DD_SERVICE_MONITORING_CONFIG_HTTP2_ENABLED=true \
  datadog/agent:7 >/dev/null
sleep 40
# Traffic between lab containers, over TCP, after system-probe has attached.
docker run -d --name netlab-client-pg --network $NET -m 200m -e PGPASSWORD=lab postgres:18-alpine sh -c 'for i in $(seq 1 200); do psql -h netlab-pg -U postgres -qtc "SELECT count(*) FROM orders WHERE customer = '"'"'c7'"'"'" -c "UPDATE orders SET amount = amount + 1 WHERE id = $i" -c "INSERT INTO orders (customer, amount) VALUES ('"'"'x'"'"', 1)" >/dev/null 2>&1; wget -qO- http://app.ninjacat.lab:8080/ping >/dev/null 2>&1; wget -qO- --header "Host: api.ninjacat.lab" http://app.ninjacat.lab:8080/api/v1/validate >/dev/null 2>&1; sleep 0.4; done' >/dev/null
docker run -d --name netlab-client-redis --network $NET -m 100m redis:7-alpine sh -c 'for i in $(seq 1 300); do redis-cli -h netlab-redis SET session:$i v$i >/dev/null; redis-cli -h netlab-redis GET session:$i >/dev/null; redis-cli -h netlab-redis INCR counter >/dev/null; redis-cli -h netlab-redis LPUSH counter x >/dev/null 2>&1; sleep 0.3; done' >/dev/null
sleep 110
docker exec netlab-agent agent status 2>/dev/null | grep -i -A14 "system probe\|System Probe" | head -30
docker exec netlab-agent agent status 2>/dev/null | grep -A10 "^    network_path" | head -12
docker logs netlab-agent 2>&1 | grep -E "SYS-PROBE|PROCESS" | grep -iE "error|warn|fail" | cut -c40-420 | sort | uniq -c | sort -rn | head -12
docker rm -f netlab-agent netlab-pg netlab-redis netlab-client-pg netlab-client-redis >/dev/null
echo "probe: done"
