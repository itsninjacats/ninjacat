//! Universal Service Monitoring: the four serialized messages inside a Connection
//! (HTTPAggregations, HTTP2Aggregations, DataStreamsAggregations, DatabaseAggregations), one row
//! per entry.
//!
//! Bytes that do not parse give no rows and an error for the log. The bytes themselves stay on the
//! `connections` row either way.
//!
//! The maps of the messages arrive unordered; their entries become rows in the order of their
//! keys.

use std::collections::HashMap;

use ninjapaws_proto::datadog::process_agent::{
    DataStreamsAggregations, DatabaseAggregations, Http2Aggregations, HttpAggregations, HttpMethod, HttpStats,
    PostgresOperation, RedisCommand, RedisErrorType, database_stats::DbStats, http_stats,
};
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::usm::{ConnectionDatabaseStat, ConnectionHttpStat, ConnectionKafkaStat, ConnectionKey};
use prost::Message;

use super::trace_sketch;
use crate::wire::enum_name;

fn by_key<V>(map: &HashMap<i32, V>) -> Vec<(i32, &V)> {
    let mut entries: Vec<(i32, &V)> = map.iter().map(|(k, v)| (*k, v)).collect();
    entries.sort_by_key(|(key, _)| *key);
    entries
}

fn status_class(code: i32) -> String {
    if (100..600).contains(&code) { format!("{}xx", code / 100) } else { String::new() }
}

/// The older shape: five counters, in the order of HTTPResponseStatus (Info, Success, Redirect,
/// ClientErr, ServerErr).
fn class_of_slot(slot: usize) -> String {
    if slot <= 4 { format!("{}xx", slot + 1) } else { String::new() }
}

fn http_stat(
    connection: &ConnectionKey,
    protocol: &str,
    stats: &HttpStats,
    status_code: i32,
    status_class: String,
    data: &http_stats::Data,
) -> ConnectionHttpStat {
    // A latency sketch that does not decode is kept as bytes and marked undecodable; it does not
    // cost the row.
    let sketch = trace_sketch::summary(&data.latencies);
    ConnectionHttpStat {
        tenant_id: connection.tenant_id.clone(),
        timestamp: connection.timestamp,
        host: connection.host.clone(),
        payload_id: connection.payload_id,
        pid: connection.pid,
        laddr_ip: connection.laddr_ip.clone(),
        laddr_port: connection.laddr_port,
        raddr_ip: connection.raddr_ip.clone(),
        raddr_port: connection.raddr_port,
        protocol: protocol.into(),
        method: enum_name(stats.method, HttpMethod::as_str_name),
        path: stats.path.clone(),
        full_path: u8::from(stats.full_path),
        status_code,
        status_class,
        count: data.count,
        first_latency_sample: data.first_latency_sample,
        latency_sum: data.latency_sum,
        latencies: Blob(sketch.raw),
        latencies_state: sketch.state,
        sketch_count: sketch.count,
        sketch_sum: sketch.sum,
        sketch_min: sketch.min,
        sketch_max: sketch.max,
        sketch_bin_keys: sketch.bin_keys,
        sketch_bin_counts: sketch.bin_counts,
    }
}

fn http_rows(connection: &ConnectionKey, protocol: &str, endpoints: &[HttpStats]) -> Vec<ConnectionHttpStat> {
    let mut found = Vec::new();
    for stats in endpoints {
        for (code, data) in by_key(&stats.stats_by_status_code) {
            found.push(http_stat(connection, protocol, stats, code, status_class(code), data));
        }
        // Every agent fills all five slots; the empty ones say nothing.
        for (slot, data) in stats.stats_by_response_status.iter().enumerate() {
            if data.count > 0 {
                found.push(http_stat(connection, protocol, stats, 0, class_of_slot(slot), data));
            }
        }
    }
    found
}

pub fn http(connection: &ConnectionKey, bytes: &[u8]) -> Result<Vec<ConnectionHttpStat>, String> {
    let aggregations = HttpAggregations::decode(bytes).map_err(|e| format!("HTTPAggregations: {e}"))?;
    Ok(http_rows(connection, "http", &aggregations.endpoint_aggregations))
}

pub fn http2(connection: &ConnectionKey, bytes: &[u8]) -> Result<Vec<ConnectionHttpStat>, String> {
    let aggregations = Http2Aggregations::decode(bytes).map_err(|e| format!("HTTP2Aggregations: {e}"))?;
    Ok(http_rows(connection, "http2", &aggregations.endpoint_aggregations))
}

pub fn kafka(connection: &ConnectionKey, bytes: &[u8]) -> Result<Vec<ConnectionKafkaStat>, String> {
    let aggregations = DataStreamsAggregations::decode(bytes).map_err(|e| format!("DataStreamsAggregations: {e}"))?;
    let mut found = Vec::new();
    for aggregation in &aggregations.kafka_aggregations {
        let header = aggregation.header.unwrap_or_default();
        let row = |error_code: i32, count: u32, first: f64, latencies: &[u8]| {
            let sketch = trace_sketch::summary(latencies);
            ConnectionKafkaStat {
                tenant_id: connection.tenant_id.clone(),
                timestamp: connection.timestamp,
                host: connection.host.clone(),
                payload_id: connection.payload_id,
                pid: connection.pid,
                laddr_ip: connection.laddr_ip.clone(),
                laddr_port: connection.laddr_port,
                raddr_ip: connection.raddr_ip.clone(),
                raddr_port: connection.raddr_port,
                api_key: header.request_type,
                api_version: header.request_version,
                topic: aggregation.topic.clone(),
                error_code,
                count,
                first_latency_sample: first,
                latencies: Blob(sketch.raw),
                latencies_state: sketch.state,
                sketch_count: sketch.count,
                sketch_sum: sketch.sum,
                sketch_min: sketch.min,
                sketch_max: sketch.max,
                sketch_bin_keys: sketch.bin_keys,
                sketch_bin_counts: sketch.bin_counts,
            }
        };
        for (code, stats) in by_key(&aggregation.stats_by_error_code) {
            found.push(row(code, stats.count, stats.first_latency_sample, &stats.latencies));
        }
        // Agents older than error codes sent one count per topic; it is deprecated in the schema
        // and still read, since those agents send nothing else.
        #[allow(deprecated)]
        let count = aggregation.count;
        if aggregation.stats_by_error_code.is_empty() && count > 0 {
            found.push(row(0, count, 0.0, &[]));
        }
    }
    Ok(found)
}

/// A database row's columns from the connection, the operation and its latencies; the rest is
/// filled in by the caller.
fn database_stat(
    connection: &ConnectionKey,
    dbms: &str,
    operation: String,
    latencies: &[u8],
) -> ConnectionDatabaseStat {
    let sketch = trace_sketch::summary(latencies);
    ConnectionDatabaseStat {
        tenant_id: connection.tenant_id.clone(),
        timestamp: connection.timestamp,
        host: connection.host.clone(),
        payload_id: connection.payload_id,
        pid: connection.pid,
        laddr_ip: connection.laddr_ip.clone(),
        laddr_port: connection.laddr_port,
        raddr_ip: connection.raddr_ip.clone(),
        raddr_port: connection.raddr_port,
        dbms: dbms.into(),
        operation,
        table_name: String::new(),
        key_name: String::new(),
        key_truncated: 0,
        error_type: String::new(),
        count: 0,
        first_latency_sample: 0.0,
        latencies: Blob(sketch.raw),
        latencies_state: sketch.state,
        sketch_count: sketch.count,
        sketch_sum: sketch.sum,
        sketch_min: sketch.min,
        sketch_max: sketch.max,
        sketch_bin_keys: sketch.bin_keys,
        sketch_bin_counts: sketch.bin_counts,
    }
}

pub fn database(connection: &ConnectionKey, bytes: &[u8]) -> Result<Vec<ConnectionDatabaseStat>, String> {
    let aggregations = DatabaseAggregations::decode(bytes).map_err(|e| format!("DatabaseAggregations: {e}"))?;
    let mut found = Vec::new();
    for stats in &aggregations.aggregations {
        match &stats.db_stats {
            Some(DbStats::Postgres(postgres)) => {
                let operation = enum_name(postgres.operation, PostgresOperation::as_str_name);
                found.push(ConnectionDatabaseStat {
                    table_name: postgres.table_name.clone(),
                    count: postgres.count,
                    first_latency_sample: postgres.first_latency_sample,
                    ..database_stat(connection, "postgres", operation, &postgres.latencies)
                });
            }
            Some(DbStats::Redis(redis)) => {
                for (error, entry) in by_key(&redis.error_to_stats) {
                    let command = enum_name(redis.command, RedisCommand::as_str_name);
                    found.push(ConnectionDatabaseStat {
                        key_name: redis.key_name.clone(),
                        key_truncated: u8::from(redis.truncated),
                        error_type: enum_name(error, RedisErrorType::as_str_name),
                        count: entry.count,
                        first_latency_sample: entry.first_latency_sample,
                        ..database_stat(connection, "redis", command, &entry.latencies)
                    });
                }
            }
            None => {}
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routers::trace_sketch::tests::go_sketch;
    use chrono::TimeZone;
    use ninjapaws_proto::datadog::process_agent::{
        DatabaseStats, KafkaAggregation, KafkaRequestHeader, KafkaStats, PostgresStats, RedisStats, RedisStatsEntry,
    };
    use ninjapaws_storage::rows::traces::SketchState;

    pub(crate) fn key() -> ConnectionKey {
        ConnectionKey {
            tenant_id: "t".into(),
            timestamp: chrono::Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap(),
            host: "host-a".into(),
            payload_id: "11111111-1111-1111-1111-111111111111".parse().unwrap(),
            pid: 42,
            laddr_ip: "10.0.0.1".into(),
            laddr_port: 5000,
            raddr_ip: "10.0.0.2".into(),
            raddr_port: 443,
        }
    }

    fn data(count: u32, latencies: Vec<u8>, first: f64, sum: f64) -> http_stats::Data {
        http_stats::Data { count, latencies, first_latency_sample: first, latency_sum: sum }
    }

    pub(crate) fn http_aggregations() -> HttpAggregations {
        let orders = HttpStats {
            path: "/orders".into(),
            method: HttpMethod::Post as i32,
            full_path: true,
            stats_by_status_code: HashMap::from([
                (201, data(3, go_sketch(), 0.0, 0.0)),
                (503, data(1, Vec::new(), 7e6, 0.0)),
            ]),
            ..Default::default()
        };
        // An older agent: five slots by class, only the 4xx one filled.
        let legacy = HttpStats {
            path: "/old".into(),
            method: HttpMethod::Get as i32,
            stats_by_response_status: [0, 0, 0, 2, 0].map(|count| data(count, Vec::new(), 0.0, 9e6)).to_vec(),
            ..Default::default()
        };
        HttpAggregations { endpoint_aggregations: vec![orders, legacy] }
    }

    #[allow(deprecated)]
    pub(crate) fn kafka_aggregations() -> DataStreamsAggregations {
        let header = |request_type, request_version| Some(KafkaRequestHeader { request_type, request_version });
        let stats = |count, latencies, first| KafkaStats { count, latencies, first_latency_sample: first };
        let produce = KafkaAggregation {
            header: header(0, 9),
            topic: "orders".into(),
            stats_by_error_code: HashMap::from([(0, stats(5, go_sketch(), 0.0)), (-1, stats(1, Vec::new(), 2e6))]),
            count: 0,
        };
        let old = KafkaAggregation { header: header(1, 4), topic: "audit".into(), count: 8, ..Default::default() };
        DataStreamsAggregations { kafka_aggregations: vec![produce, old] }
    }

    pub(crate) fn database_aggregations() -> DatabaseAggregations {
        let select = PostgresStats {
            table_name: "orders".into(),
            operation: PostgresOperation::PostgresSelectOp as i32,
            count: 4,
            latencies: go_sketch(),
            ..Default::default()
        };
        let entry = |count, latencies, first| RedisStatsEntry { count, latencies, first_latency_sample: first };
        let get = RedisStats {
            command: RedisCommand::RedisGetCommand as i32,
            key_name: "session:1".into(),
            truncated: true,
            error_to_stats: HashMap::from([
                (RedisErrorType::RedisNoError as i32, entry(10, go_sketch(), 0.0)),
                (RedisErrorType::RedisErrOom as i32, entry(1, Vec::new(), 3e6)),
            ]),
        };
        DatabaseAggregations {
            aggregations: vec![
                DatabaseStats { db_stats: Some(DbStats::Postgres(select)) },
                DatabaseStats { db_stats: Some(DbStats::Redis(get)) },
            ],
        }
    }

    #[test]
    fn an_http_endpoint_gives_one_row_per_status_code_or_per_class_from_an_older_agent() {
        let stats = http(&key(), &http_aggregations().encode_to_vec()).unwrap();
        let mut seen: Vec<_> = stats
            .iter()
            .map(|s| (s.path.as_str(), s.method.as_str(), s.status_code, s.status_class.as_str(), s.count))
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            vec![("/old", "Get", 0, "4xx", 2), ("/orders", "Post", 201, "2xx", 3), ("/orders", "Post", 503, "5xx", 1)]
        );
        assert!(stats.iter().all(|s| s.protocol == "http" && s.payload_id == key().payload_id && s.pid == 42));

        let created = stats.iter().find(|s| s.status_code == 201).unwrap();
        assert_eq!(created.full_path, 1);
        assert_eq!((&created.latencies_state, created.sketch_count), (&SketchState::Ok, Some(3.0)));

        // One sample: the latency is in the field, and there is no sketch.
        let failed = stats.iter().find(|s| s.status_code == 503).unwrap();
        assert_eq!((failed.first_latency_sample, &failed.latencies_state), (7e6, &SketchState::Absent));

        let old = stats.iter().find(|s| s.path == "/old").unwrap();
        assert_eq!(old.latency_sum, 9e6);
    }

    #[test]
    fn http2_has_the_same_shape_under_its_own_message() {
        let aggregations = Http2Aggregations { endpoint_aggregations: http_aggregations().endpoint_aggregations };
        let stats = http2(&key(), &aggregations.encode_to_vec()).unwrap();
        assert_eq!(stats.len(), 3);
        assert!(stats.iter().all(|s| s.protocol == "http2"));
    }

    #[test]
    fn a_kafka_topic_gives_one_row_per_error_code_and_one_for_an_agent_that_sent_only_a_count() {
        let stats = kafka(&key(), &kafka_aggregations().encode_to_vec()).unwrap();
        let mut seen: Vec<_> =
            stats.iter().map(|s| (s.topic.as_str(), s.api_key, s.api_version, s.error_code, s.count)).collect();
        seen.sort();
        assert_eq!(seen, vec![("audit", 1, 4, 0, 8), ("orders", 0, 9, -1, 1), ("orders", 0, 9, 0, 5)]);
        let ok = stats.iter().find(|s| s.topic == "orders" && s.error_code == 0).unwrap();
        assert_eq!(ok.latencies_state, SketchState::Ok);
    }

    #[test]
    fn a_postgres_operation_is_one_row_a_redis_command_one_row_per_error_type() {
        let stats = database(&key(), &database_aggregations().encode_to_vec()).unwrap();
        let mut seen: Vec<_> = stats
            .iter()
            .map(|s| {
                (
                    s.dbms.as_str(),
                    s.operation.as_str(),
                    s.table_name.as_str(),
                    s.key_name.as_str(),
                    s.error_type.as_str(),
                    s.count,
                )
            })
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            vec![
                ("postgres", "PostgresSelectOp", "orders", "", "", 4),
                ("redis", "RedisGetCommand", "", "session:1", "RedisErrOom", 1),
                ("redis", "RedisGetCommand", "", "session:1", "RedisNoError", 10),
            ]
        );
        let mut truncated: Vec<u8> = stats.iter().map(|s| s.key_truncated).collect();
        truncated.sort();
        assert_eq!(truncated, vec![0, 1, 1]);
    }

    #[test]
    fn no_bytes_give_no_rows_bytes_that_are_not_the_message_give_an_error() {
        assert!(http(&key(), &[]).unwrap().is_empty());
        assert!(kafka(&key(), &[]).unwrap().is_empty());
        assert!(database(&key(), &[]).unwrap().is_empty());

        let garbage = [0xff, 0xff, 0xff];
        assert!(http(&key(), &garbage).is_err());
        assert!(http2(&key(), &garbage).is_err());
        assert!(kafka(&key(), &garbage).is_err());
        assert!(database(&key(), &garbage).is_err());
    }

    #[test]
    fn a_sketch_that_is_not_a_sketch_is_kept_as_bytes_and_does_not_cost_the_row() {
        let stats = HttpStats {
            path: "/x".into(),
            method: HttpMethod::Get as i32,
            stats_by_status_code: HashMap::from([(200, data(2, vec![0xff, 0x01], 0.0, 0.0))]),
            ..Default::default()
        };
        let rows = http(&key(), &HttpAggregations { endpoint_aggregations: vec![stats] }.encode_to_vec()).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0].latencies_state, SketchState::Undecodable(_)));
        assert_eq!(rows[0].latencies.0, vec![0xff, 0x01]);
    }

    /// Bytes system-probe 7.84.0 really put inside connections between lab containers: the agent
    /// posting to the intake, psql against Postgres, redis-cli against Redis.
    fn recorded(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real").join(name);
        std::fs::read(path).unwrap()
    }

    #[test]
    fn real_http_aggregations_give_the_endpoints_the_agent_called_by_status_code() {
        let stats = http(&key(), &recorded("system-probe-7.84.0-http-aggregations.bin")).unwrap();
        let mut paths: Vec<&str> = stats.iter().map(|s| s.path.as_str()).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths, vec!["/api/v1/check_run", "/api/v1/metadata", "/api/v2/series", "/intake/"]);
        for s in &stats {
            assert_eq!((s.protocol.as_str(), s.method.as_str(), s.status_class.as_str()), ("http", "Post", "2xx"));
            assert!(s.status_code == 200 || s.status_code == 202);
            assert!(s.count > 0);
            // One sample is sent as a number, more as a sketch that decodes.
            if s.count == 1 {
                assert_eq!(s.latencies_state, SketchState::Absent);
                assert!(s.first_latency_sample > 0.0);
            } else {
                assert_eq!((&s.latencies_state, s.sketch_count), (&SketchState::Ok, Some(f64::from(s.count))));
            }
        }
    }

    #[test]
    fn real_postgres_aggregations_name_the_operation_and_the_table() {
        let stats = database(&key(), &recorded("system-probe-7.84.0-postgres-aggregations.bin")).unwrap();
        let mut seen: Vec<_> =
            stats.iter().map(|s| (s.dbms.as_str(), s.operation.as_str(), s.table_name.as_str())).collect();
        seen.sort();
        assert_eq!(
            seen,
            vec![
                ("postgres", "PostgresInsertOp", "orders"),
                ("postgres", "PostgresSelectOp", "orders"),
                ("postgres", "PostgresUpdateOp", "orders"),
            ]
        );
        assert!(stats.iter().all(|s| s.count == 1 && s.first_latency_sample > 0.0));
    }

    #[test]
    fn real_redis_aggregations_name_the_command_the_key_is_sent_only_when_the_agent_is_told_to() {
        let stats = database(&key(), &recorded("system-probe-7.84.0-redis-aggregations.bin")).unwrap();
        assert!(!stats.is_empty());
        for s in &stats {
            assert_eq!((s.dbms.as_str(), s.key_name.as_str(), s.error_type.as_str()), ("redis", "", "RedisNoError"));
            assert!(["RedisGetCommand", "RedisSetCommand"].contains(&s.operation.as_str()));
        }
    }

    /// The rows inserted through the real writers into a scratch database built from the
    /// migrations. Passes without asserting when ClickHouse is not running.
    #[tokio::test]
    async fn the_stat_rows_fit_their_clickhouse_tables() {
        let server = ninjapaws_storage::client_from_env().with_database("default");
        if server.query("SELECT 1").execute().await.is_err() {
            eprintln!("ClickHouse is not running: the round trip is skipped");
            return;
        }
        let database_name = format!("ninjapaws_usm_{}", std::process::id());
        server.query(&format!("CREATE DATABASE {database_name}")).execute().await.unwrap();
        let client = ninjapaws_storage::client_from_env().with_database(&database_name);
        ninjapaws_storage::migrations::apply(&client).await.unwrap();

        let sink = ninjapaws_storage::Sink::clickhouse(client);
        let inserted = async {
            sink.insert(http(&key(), &http_aggregations().encode_to_vec()).unwrap()).await?;
            sink.insert(kafka(&key(), &kafka_aggregations().encode_to_vec()).unwrap()).await?;
            sink.insert(database(&key(), &database_aggregations().encode_to_vec()).unwrap()).await
        }
        .await;
        server.query(&format!("DROP DATABASE {database_name}")).execute().await.unwrap();
        inserted.unwrap();
    }
}
