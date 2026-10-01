#!/bin/bash
# An SNMP device, traps and NetFlow against a real agent; then what it sent.
N=$(dirname "$(readlink -f "$0")")
KEY=${LAB_KEY:-1ab00000000000000000000000000001}
NET=ninjacat-lab
docker rm -f netlab-snmp netlab-agent >/dev/null 2>&1
docker build -q -t ninjacat/lab-snmp $N/snmp >/dev/null
docker run -d --name netlab-snmp --network $NET -m 200m ninjacat/lab-snmp >/dev/null
sleep 2
IP=$(docker inspect netlab-snmp --format '{{(index .NetworkSettings.Networks "ninjacat-lab").IPAddress}}')
cat > $N/conf/snmp.d/conf.yaml <<YAML
init_config:
  loader: core
  use_device_id_as_hostname: true
instances:
  - ip_address: $IP
    community_string: public
    snmp_version: 2
    collect_topology: true
    profile: generic-device
    min_collection_interval: 15
    tags: ["team:core"]
YAML
chmod -R a+rX $N
docker run -d --name netlab-agent --network $NET -m 1g \
  -v $N/datadog.yaml:/etc/datadog-agent/datadog.yaml:ro -v $N/conf/snmp.d/conf.yaml:/etc/datadog-agent/conf.d/snmp.d/conf.yaml:ro -v $N/netflow.py:/netflow.py:ro \
  -e DD_API_KEY=$KEY -e DD_HOSTNAME=netlab-agent -e DD_PROCESS_AGENT_ENABLED=false datadog/agent:7 >/dev/null
sleep 45
docker exec netlab-snmp snmptrap -v 2c -c public netlab-agent:9162 '' 1.3.6.1.6.3.1.1.5.3 1.3.6.1.2.1.2.2.1.1.1 i 1 1.3.6.1.2.1.2.2.1.7.1 i 2 1.3.6.1.2.1.2.2.1.8.1 i 2
docker exec netlab-snmp snmptrap -v 2c -c public netlab-agent:9162 '' 1.3.6.1.6.3.1.1.5.1
docker exec netlab-snmp snmptrap -v 2c -c public netlab-agent:9162 '' 1.3.6.1.4.1.99999.0.7 1.3.6.1.4.1.99999.1.1 s 'fan tray 2 failed' 1.3.6.1.4.1.99999.1.2 i 42
docker exec netlab-agent python3 /netflow.py
sleep 45
docker exec netlab-agent agent status 2>/dev/null | grep -A10 "^    snmp" | head -12
docker exec netlab-agent agent status 2>/dev/null | grep -i -A8 "netflow\|SNMP Traps" | head -30
docker logs netlab-agent 2>&1 | grep -iE "snmp|netflow|trap|ndm" | grep -iE "error|warn" | cut -c80-600 | sort | uniq -c | sort -rn | head -8
docker rm -f netlab-agent netlab-snmp >/dev/null
echo "netlab: done"
