//! The ClickHouse schema: the numbered SQL files of `api/schema/migrations`, embedded in the
//! binary, applied in order and recorded in `schema_migrations`. The F# server applies the same
//! files into the same ledger, with the same checksums.
//!
//! Applied migrations are immutable. Each is recorded with a checksum of its file, and a file that
//! changed after it was applied stops the run rather than letting the schema drift from the files.

use std::collections::HashMap;

use include_dir::{Dir, include_dir};

static FILES: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../api/schema/migrations");

#[derive(Clone, Debug)]
pub struct Migration {
    pub version: u32,
    pub name: String,
    pub sql: String,
    pub checksum: u64,
}

const CREATE_LEDGER: &str = "
CREATE TABLE IF NOT EXISTS schema_migrations (
    version    UInt32,
    name       String,
    applied_at DateTime64(3, 'UTC') DEFAULT now64(3),
    checksum   UInt64
) ENGINE = ReplacingMergeTree(applied_at) ORDER BY version";

/// A migration file's checksum. Ledgers already hold checksums written with this function, so it
/// cannot change.
pub fn checksum(data: &[u8]) -> u64 {
    crate::fnv::hash64(data)
}

pub fn parse_filename(name: &str) -> Result<(u32, String), String> {
    let pattern = regex::Regex::new(r"^(\d{4})_([a-z0-9][a-z0-9_]*)\.sql$").unwrap();
    let Some(found) = pattern.captures(name) else {
        return Err(format!("schema: migration filename \"{name}\" does not match NNNN_snake_name.sql"));
    };
    match found[1].parse::<u32>() {
        Ok(0) | Err(_) => Err(format!("schema: migration filename \"{name}\": version 0 is not a valid version")),
        Ok(version) => Ok((version, found[2].to_string())),
    }
}

/// Splits a migration file into statements at each `;` outside quotes and comments. Comments are
/// dropped: ClickHouse over HTTP takes one statement per request and refuses a request that is
/// only a comment.
pub fn split_statements(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut flush = |current: &mut String| {
        let statement = current.trim();
        if !statement.is_empty() {
            statements.push(statement.to_string());
        }
        current.clear();
    };

    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '\'' || c == '`' || c == '"' {
            // A quoted run, copied whole; a backslash keeps the next character.
            current.push(c);
            i += 1;
            let mut closed = false;
            while i < n && !closed {
                if chars[i] == '\\' && i + 1 < n {
                    current.push(chars[i]);
                    current.push(chars[i + 1]);
                    i += 2;
                } else {
                    current.push(chars[i]);
                    closed = chars[i] == c;
                    i += 1;
                }
            }
        } else if c == '-' && i + 1 < n && chars[i + 1] == '-' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            i += 2;
            let mut closed = false;
            while i < n && !closed {
                if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                    i += 2;
                    closed = true;
                } else {
                    i += 1;
                }
            }
            current.push(' ');
        } else if c == ';' {
            flush(&mut current);
            i += 1;
        } else {
            current.push(c);
            i += 1;
        }
    }
    flush(&mut current);
    statements
}

/// Every embedded migration, by version.
pub fn load() -> Result<Vec<Migration>, String> {
    let mut migrations = Vec::new();
    for file in FILES.files() {
        let name = file.path().file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let (version, title) = parse_filename(name)?;
        let sql = String::from_utf8(file.contents().to_vec()).map_err(|_| format!("schema: {name} is not UTF-8"))?;
        migrations.push(Migration { version, name: title, sql, checksum: checksum(file.contents()) });
    }
    migrations.sort_by_key(|m| m.version);
    if let Some(pair) = migrations.windows(2).find(|pair| pair[0].version == pair[1].version) {
        return Err(format!(
            "schema: two migrations share version {:04} ({} and {})",
            pair[0].version, pair[0].name, pair[1].name
        ));
    }
    Ok(migrations)
}

/// The migrations still to apply, given what the ledger says was applied.
pub fn plan(migrations: &[Migration], applied: &HashMap<u32, u64>) -> Result<Vec<Migration>, String> {
    let changed = migrations.iter().find(|m| applied.get(&m.version).is_some_and(|recorded| *recorded != m.checksum));
    if let Some(m) = changed {
        return Err(format!(
            "schema: migration {:04}_{} has already been applied but its file has changed \
             (ledger checksum {}, file checksum {}). Applied migrations are immutable: the edited SQL \
             will never run here and the schema would silently diverge from the files. Revert the \
             edit and add a new migration instead",
            m.version, m.name, applied[&m.version], m.checksum
        ));
    }
    Ok(migrations.iter().filter(|m| !applied.contains_key(&m.version)).cloned().collect())
}

/// Applies what is pending; returns how many migrations ran.
pub async fn apply(client: &clickhouse::Client) -> Result<usize, String> {
    let migrations = load()?;
    client.query(CREATE_LEDGER).execute().await.map_err(|e| format!("schema: ledger: {e}"))?;
    let applied: HashMap<u32, u64> = client
        .query("SELECT version, checksum FROM schema_migrations FINAL")
        .fetch_all::<(u32, u64)>()
        .await
        .map_err(|e| format!("schema: ledger: {e}"))?
        .into_iter()
        .collect();

    let pending = plan(&migrations, &applied)?;
    for m in &pending {
        let fail = |e: clickhouse::error::Error| format!("schema: migration {:04}_{}: {e}", m.version, m.name);
        for statement in split_statements(&m.sql) {
            client.query(&statement).execute().await.map_err(fail)?;
        }
        client
            .query("INSERT INTO schema_migrations (version, name, checksum) VALUES (?, ?, ?)")
            .bind(m.version)
            .bind(&m.name)
            .bind(m.checksum)
            .execute()
            .await
            .map_err(fail)?;
    }
    Ok(pending.len())
}

/// The TTLs of `metrics`, `metrics_1m` and `metric_tag_keys`, from the configured retention, after
/// the migrations: a migration cannot read configuration, and an applied one never changes. Only
/// the table's metadata changes; parts already written are not rewritten, and take the new TTL as
/// they merge.
pub async fn set_metrics_retention(client: &clickhouse::Client, raw_days: i64, rollup_days: i64) -> Result<(), String> {
    let statements = [
        format!(
            "ALTER TABLE metrics MODIFY TTL toDateTime(timestamp) + INTERVAL {raw_days} DAY SETTINGS materialize_ttl_after_modify = 0"
        ),
        format!(
            "ALTER TABLE metrics_1m MODIFY TTL minute + INTERVAL {rollup_days} DAY SETTINGS materialize_ttl_after_modify = 0"
        ),
        // A metric is listed as long as the rollup can answer for it.
        format!(
            "ALTER TABLE metric_tag_keys MODIFY TTL hour + INTERVAL {rollup_days} DAY SETTINGS materialize_ttl_after_modify = 0"
        ),
    ];
    for statement in statements {
        client.query(&statement).execute().await.map_err(|e| format!("schema: metrics retention: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_migration_loads_and_the_first_is_the_initial_schema() {
        let migrations = load().unwrap();
        assert_eq!(migrations[0].version, 1);
        assert_eq!(migrations[0].name, "initial");
    }

    #[test]
    fn filenames() {
        assert_eq!(parse_filename("0002_metrics_rollup.sql"), Ok((2, "metrics_rollup".into())));
        assert!(parse_filename("0000_zero.sql").is_err());
        assert!(parse_filename("2_short.sql").is_err());
        assert!(parse_filename("0003_Upper.sql").is_err());
    }

    #[test]
    fn statements_split_outside_quotes_and_comments() {
        let sql = "-- a comment; still a comment\nCREATE TABLE a (x String DEFAULT 'a;b') ENGINE = Memory;\n/* ; */ SELECT 1;";
        assert_eq!(split_statements(sql), vec!["CREATE TABLE a (x String DEFAULT 'a;b') ENGINE = Memory", "SELECT 1"]);
    }

    #[test]
    fn an_edited_applied_migration_stops_the_plan() {
        let migrations = vec![
            Migration { version: 1, name: "one".into(), sql: String::new(), checksum: 1 },
            Migration { version: 2, name: "two".into(), sql: String::new(), checksum: 2 },
        ];
        let pending = plan(&migrations, &HashMap::from([(1, 1)])).unwrap();
        assert_eq!(pending.iter().map(|m| m.version).collect::<Vec<_>>(), vec![2]);
        assert!(plan(&migrations, &HashMap::from([(1, 9)])).is_err());
    }

    #[test]
    fn the_checksum_is_fnv_1a() {
        assert_eq!(checksum(b""), 14695981039346656037);
        assert_eq!(checksum(b"a"), 0xaf63dc4c8601ec8c);
    }
}
