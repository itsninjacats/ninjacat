#!/bin/bash
# Runs one database with a real agent and its DBM integration against the lab server
# (lab/server.sh up). Usage: run.sh postgres|mysql|mariadb|sqlserver|oracle|mongo|clickhouse
set -u
L=$(dirname "$(readlink -f "$0")")
KEY=${LAB_KEY:-1ab00000000000000000000000000001}
NET=ninjacat-lab
db=$1

say() { echo "[$(date +%H:%M:%S)] $db: $*"; }

docker rm -f dbmlab-db dbmlab-agent >/dev/null 2>&1

case $db in
  mysql)
    integration=mysql
    docker run -d --name dbmlab-db --network $NET -m 1200m -e MYSQL_ROOT_PASSWORD=lab -v $L/mysql/init:/docker-entrypoint-initdb.d:ro mysql:8.4 \
      --performance-schema-consumer-events-statements-current=ON --performance-schema-consumer-events-waits-current=ON \
      --performance-schema-consumer-events-statements-history-long=ON --performance-schema-consumer-events-statements-history=ON \
      --max-digest-length=4096 --performance-schema-max-digest-length=4096 --performance-schema-max-sql-text-length=4096 >/dev/null
    ready() { docker exec dbmlab-db mysql -uroot -plab -N -e "SELECT COUNT(*) FROM shop.orders" 2>/dev/null | grep -q 1000; }
    load() { docker exec dbmlab-db mysql -uroot -plab shop -e "SELECT COUNT(*) FROM orders WHERE customer='c7'; SELECT SUM(amount) FROM orders WHERE amount > $1; UPDATE orders SET amount = amount + 1 WHERE id = $1; SELECT SLEEP(0.7);" >/dev/null 2>&1; }
    ;;
  mariadb)
    integration=mysql
    docker run -d --name dbmlab-db --network $NET -m 1200m -e MARIADB_ROOT_PASSWORD=lab -v $L/mariadb/init:/docker-entrypoint-initdb.d:ro mariadb:11 \
      --performance-schema=ON --performance-schema-consumer-events-statements-current=ON --performance-schema-consumer-events-waits-current=ON \
      --performance-schema-consumer-events-statements-history-long=ON --performance-schema-consumer-events-statements-history=ON \
      --max-digest-length=4096 --performance-schema-max-digest-length=4096 --performance-schema-max-sql-text-length=4096 >/dev/null
    ready() { docker exec dbmlab-db mariadb -uroot -plab -N -e "SELECT COUNT(*) FROM shop.orders" 2>/dev/null | grep -q 1000; }
    load() { docker exec dbmlab-db mariadb -uroot -plab shop -e "SELECT COUNT(*) FROM orders WHERE customer='c7'; SELECT SUM(amount) FROM orders WHERE amount > $1; UPDATE orders SET amount = amount + 1 WHERE id = $1; SELECT SLEEP(0.7);" >/dev/null 2>&1; }
    ;;
  sqlserver)
    integration=sqlserver
    docker run -d --name dbmlab-db --network $NET -m 2500m -e ACCEPT_EULA=Y -e MSSQL_SA_PASSWORD='Lab-Pass-12345' -e MSSQL_PID=Developer \
      -e MSSQL_MEMORY_LIMIT_MB=1800 -e MSSQL_AGENT_ENABLED=true mcr.microsoft.com/mssql/server:2022-latest >/dev/null
    sql() { docker exec dbmlab-db /opt/mssql-tools18/bin/sqlcmd -S localhost -U sa -P 'Lab-Pass-12345' -C -b "$@"; }
    initialised=0
    ready() {
      sql -Q "SELECT 1" >/dev/null 2>&1 || return 1
      if [ $initialised = 0 ]; then docker cp $L/sqlserver/init.sql dbmlab-db:/tmp/init.sql >/dev/null && sql -i /tmp/init.sql >/dev/null 2>&1; initialised=1; fi
      sql -d shop -Q "SELECT COUNT(*) FROM orders" 2>/dev/null | grep -q 1000
    }
    load() { sql -d shop -Q "SELECT COUNT(*) FROM orders WHERE customer='c7'; SELECT SUM(amount) FROM orders WHERE amount > $1; UPDATE orders SET amount = amount + 1 WHERE id = $1; EXEC dbo.customer_total 'c7'; WAITFOR DELAY '00:00:00.700';" >/dev/null 2>&1; }
    ;;
  oracle)
    integration=oracle
    docker run -d --name dbmlab-db --network $NET -m 2500m -e ORACLE_PASSWORD=lab gvenzl/oracle-free:23-slim >/dev/null
    initialised=0
    ready() {
      docker logs dbmlab-db 2>&1 | grep -q "DATABASE IS READY TO USE" || return 1
      if [ $initialised = 0 ]; then docker exec -i dbmlab-db sqlplus -s / as sysdba < $L/oracle/init.sql >/dev/null 2>&1; initialised=1; fi
      echo "SELECT COUNT(*) FROM shop.orders;" | docker exec -i dbmlab-db sqlplus -s shop/shop@//localhost/FREEPDB1 2>/dev/null | grep -q 1000
    }
    load() { printf "SELECT COUNT(*) FROM orders WHERE customer='c7';\nSELECT SUM(amount) FROM orders WHERE amount > $1;\nUPDATE orders SET amount = amount + 1 WHERE id = $1;\nCOMMIT;\nSELECT COUNT(*) FROM orders a, orders b WHERE a.amount > b.amount;\n" | docker exec -i dbmlab-db sqlplus -s shop/shop@//localhost/FREEPDB1 >/dev/null 2>&1; }
    ;;
  mongo)
    integration=mongo
    docker run -d --name dbmlab-db --network $NET -m 1200m mongo:8 --replSet rs0 --bind_ip_all >/dev/null
    js() { docker exec dbmlab-db mongosh --quiet --eval "$1" 2>/dev/null; }
    initialised=0
    ready() {
      js "db.runCommand({ping:1}).ok" | grep -q 1 || return 1
      if [ $initialised = 0 ]; then
        js 'rs.initiate({_id:"rs0",members:[{_id:0,host:"dbmlab-db:27017"}]})' >/dev/null; sleep 8
        js 'db.getSiblingDB("admin").createUser({user:"datadog",pwd:"datadog",roles:[{role:"read",db:"admin"},{role:"clusterMonitor",db:"admin"},{role:"read",db:"local"},{role:"read",db:"shop"}]})' >/dev/null
        js 'const s=db.getSiblingDB("shop"); s.orders.insertMany(Array.from({length:1000},(_,i)=>({customer:"c"+(i%50),amount:i*1.5,items:[{sku:"a",qty:i%3}]}))); s.orders.createIndex({customer:1}); s.setProfilingLevel(1,{slowms:20})' >/dev/null
        initialised=1
      fi
      js 'db.getSiblingDB("shop").orders.countDocuments()' | grep -q 1000
    }
    load() { js "const s=db.getSiblingDB('shop'); s.orders.find({customer:'c7'}).toArray(); s.orders.aggregate([{\$match:{amount:{\$gt:$1}}},{\$group:{_id:'\$customer',t:{\$sum:'\$amount'}}}]).toArray(); s.orders.updateOne({amount:$1},{\$inc:{amount:1}}); s.orders.find({\$where:'sleep(2)||true'}).limit(400).toArray().length" >/dev/null; }
    ;;
  clickhouse)
    integration=clickhouse
    docker run -d --name dbmlab-db --network $NET -m 1800m -e CLICKHOUSE_USER=datadog -e CLICKHOUSE_PASSWORD=datadog -e CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT=1 \
      clickhouse/clickhouse-server:26.8.6.5 >/dev/null
    q() { docker exec dbmlab-db clickhouse-client -u datadog --password datadog -q "$1" 2>/dev/null; }
    initialised=0
    ready() {
      q "SELECT 1" | grep -q 1 || return 1
      if [ $initialised = 0 ]; then q "CREATE DATABASE IF NOT EXISTS shop"; q "CREATE TABLE shop.orders (id UInt32, customer String, amount Float64) ENGINE = MergeTree ORDER BY id"; q "INSERT INTO shop.orders SELECT number, concat('c', toString(number % 50)), number * 1.5 FROM numbers(1000)"; initialised=1; fi
      q "SELECT count() FROM shop.orders" | grep -q 1000
    }
    load() { q "SELECT count() FROM shop.orders WHERE customer = 'c7'"; q "SELECT sum(amount) FROM shop.orders WHERE amount > $1"; q "SELECT sleep(0.7), count() FROM shop.orders" ; q "SELECT no_such_column FROM shop.orders"; } >/dev/null
    ;;
  postgres)
    integration=postgres
    docker run -d --name dbmlab-db --network $NET -m 600m -e POSTGRES_PASSWORD=lab -v $L/postgres/init:/docker-entrypoint-initdb.d:ro postgres:18-alpine \
      postgres -c shared_preload_libraries=pg_stat_statements -c track_activity_query_size=4096 -c pg_stat_statements.track=all -c track_io_timing=on >/dev/null
    ready() { docker exec dbmlab-db psql -U postgres -d shop -tc "SELECT count(*) FROM orders" 2>/dev/null | grep -q 5000; }
    load() { docker exec dbmlab-db psql -U postgres -d shop -qc "SELECT count(*) FROM orders WHERE customer = 'c7'" -c "SELECT sum(amount) FROM orders WHERE amount > $1" -c "UPDATE orders SET amount = amount + 1 WHERE id = $1" -c "SELECT pg_sleep(0.7)" >/dev/null 2>&1; }
    ;;
  *) echo "unknown database $db"; exit 2 ;;
esac

for i in $(seq 1 90); do ready && break; sleep 4; done
ready || { say "database never became ready"; docker logs --tail 15 dbmlab-db 2>&1; docker rm -f dbmlab-db >/dev/null; exit 1; }
say "database ready"

docker run -d --name dbmlab-agent --network $NET -m 1g \
  -v $L/$db/conf:/etc/datadog-agent/conf.d/$integration.d:ro \
  -e DD_API_KEY=$KEY -e DD_HOSTNAME=dbmlab-$db -e DD_LOG_LEVEL=warn \
  -e DD_DD_URL=http://app.ninjacat.lab:8080 -e DD_APM_ENABLED=false -e DD_PROCESS_AGENT_ENABLED=false \
  -e DD_LOGS_CONFIG_LOGS_DD_URL=agent-http-intake.logs.ninjacat.lab:8080 -e DD_LOGS_CONFIG_USE_HTTP=true -e DD_LOGS_CONFIG_LOGS_NO_SSL=true \
  -e DD_DATABASE_MONITORING_METRICS_LOGS_DD_URL=dbm-metrics-intake.ninjacat.lab:8080 -e DD_DATABASE_MONITORING_METRICS_LOGS_NO_SSL=true \
  -e DD_DATABASE_MONITORING_ACTIVITY_LOGS_DD_URL=dbm-metrics-intake.ninjacat.lab:8080 -e DD_DATABASE_MONITORING_ACTIVITY_LOGS_NO_SSL=true \
  -e DD_DATABASE_MONITORING_SAMPLES_LOGS_DD_URL=dbm-metrics-intake.ninjacat.lab:8080 -e DD_DATABASE_MONITORING_SAMPLES_LOGS_NO_SSL=true \
  -e DD_REMOTE_CONFIGURATION_ENABLED=false datadog/agent:7 >/dev/null
sleep 20
DURATION=${DURATION:-110}
for i in $(seq 1 $((DURATION * 2))); do load $i; done &
LOADER=$!
sleep $DURATION
docker exec dbmlab-agent agent status 2>/dev/null | grep -A14 "^    $integration (" | head -18
docker logs dbmlab-agent 2>&1 | grep -iE "$integration" | grep -iE "error|warn" | cut -c80-900 | sort | uniq -c | sort -rn | head -6
kill $LOADER 2>/dev/null
docker rm -f dbmlab-agent dbmlab-db >/dev/null
say "done"
