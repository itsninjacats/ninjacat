module NinjaCat.Api.Platform.Config

open System
open NinjaCat.Api.Engine

let private envOr (key: string) (fallback: string) =
    match Environment.GetEnvironmentVariable key with
    | null
    | "" -> fallback
    | v -> v

/// A duration ("15s", "1m30s"), or a bare number of seconds.
let private duration (key: string) (fallback: TimeSpan) : TimeSpan =
    let raw = envOr key ""

    match Duration.parse raw, Int32.TryParse raw with
    | Ok d, _ when d > TimeSpan.Zero -> d
    | _, (true, seconds) when seconds > 0 -> TimeSpan.FromSeconds(float seconds)
    | _ -> fallback

/// A whole number of days, up to a century. A set value outside 1–36500
/// stops the start: falling back to the default could set a shorter TTL than
/// meant, and ClickHouse would delete the difference.
let private days (key: string) (fallback: TimeSpan) : TimeSpan =
    match envOr key "" with
    | "" -> fallback
    | raw ->
        match Int32.TryParse raw with
        | true, n when n > 0 && n <= 36500 -> TimeSpan.FromDays(float n)
        | _ -> failwith $"{key} must be a whole number of days from 1 to 36500, got \"{raw}\""

/// A step of whole minutes reads the rollup whatever its age, so a rollup
/// kept shorter than the raw points would answer nothing where the raw table
/// still holds them.
let private metricsRetention () : Query.Plan.Retention =
    let raw = days "NINJACAT_METRICS_RETENTION_DAYS" Query.Plan.defaultRetention.Raw
    let rollup = days "NINJACAT_METRICS_ROLLUP_RETENTION_DAYS" Query.Plan.defaultRetention.Rollup

    if rollup < raw then
        failwith $"NINJACAT_METRICS_ROLLUP_RETENTION_DAYS ({rollup.TotalDays}) must not be shorter than NINJACAT_METRICS_RETENTION_DAYS ({raw.TotalDays})"

    { Raw = raw; Rollup = rollup }

type Config =
    { /// Where the intake process listens (NINJACAT_ADDR), as `:8080` or
      /// `host:8080`. Reachable from agent machines.
      IntakeAddr: string
      /// Where the query process listens (NINJACAT_INTERNAL_ADDR): the panel's
      /// and the query API. Internal only: it asks for the panel's token, not
      /// for an API key.
      InternalAddr: string
      /// The key the panel signs its tokens to the query process with
      /// (NINJACAT_QUERY_TOKEN_SECRET). Only the query process needs it.
      QueryTokenSecret: string
      /// The agent's TCP transport for logs (NINJACAT_LOGS_TCP_ADDR), or
      /// None when it is "off".
      LogsTcpAddr: string option
      /// PEM files (NINJACAT_TLS_CERT, NINJACAT_TLS_KEY) that make the logs
      /// TCP listener speak TLS; both empty for plain TCP.
      TlsCertFile: string
      TlsKeyFile: string
      ClickHouseHttpAddr: string
      ClickHouseDb: string
      ClickHouseUser: string
      ClickHousePassword: string
      DatabaseUrl: string
      /// Apply the ClickHouse migrations at start-up (NINJACAT_AUTO_MIGRATE).
      /// With more than one replica, turn it off and run `migrate` as a job.
      AutoMigrate: bool
      /// Days the metrics tables keep their rows: raw points
      /// (NINJACAT_METRICS_RETENTION_DAYS, 30) and the one-minute rollup
      /// (NINJACAT_METRICS_ROLLUP_RETENTION_DAYS, 400). The migrating process
      /// sets them as the tables' TTLs, and the engine reads past the raw
      /// retention from the rollup.
      MetricsRetention: Query.Plan.Retention
      /// Reverse proxies in front of the intake (NINJACAT_TRUSTED_PROXIES):
      /// addresses or networks (`10.0.0.0/8`), comma-separated. Only what
      /// such a proxy says in X-Forwarded-For is believed.
      TrustedProxies: string list
      /// The largest request body the intake reads (NINJACAT_MAX_BODY_BYTES).
      /// A larger one is refused with 413.
      MaxBodyBytes: int64
      /// Answer 202 on unknown intake paths (NINJACAT_ACK_UNKNOWN).
      AckUnknown: bool
      /// DEBUG=true: every intake request is also dumped to CaptureDir.
      Debug: bool
      CaptureDir: string
      SelfMonitorInterval: TimeSpan
      SelfMonitorTenant: string
      SelfMonitorHost: string }

// CLICKHOUSE_HTTP_ADDR is ClickHouse's HTTP interface (:8123), which the
// driver speaks; not the native protocol's port.
let load () =
    { IntakeAddr = envOr "NINJACAT_ADDR" ":8080"
      InternalAddr = envOr "NINJACAT_INTERNAL_ADDR" ":8081"
      QueryTokenSecret = envOr "NINJACAT_QUERY_TOKEN_SECRET" ""
      LogsTcpAddr =
        match envOr "NINJACAT_LOGS_TCP_ADDR" ":10516" with
        | "off" -> None
        | address -> Some address
      TlsCertFile = envOr "NINJACAT_TLS_CERT" ""
      TlsKeyFile = envOr "NINJACAT_TLS_KEY" ""
      ClickHouseHttpAddr = envOr "CLICKHOUSE_HTTP_ADDR" "localhost:8123"
      ClickHouseDb = envOr "CLICKHOUSE_DB" "ninjacat"
      ClickHouseUser = envOr "CLICKHOUSE_USER" "ninjacat"
      ClickHousePassword = envOr "CLICKHOUSE_PASSWORD" "ninjacat"
      // Required: there is no sensible default database.
      DatabaseUrl =
        match envOr "DATABASE_URL" "" with
        | "" -> failwith "DATABASE_URL is not set (e.g. postgres://root:mysecretpassword@localhost:5433/local)"
        | url -> url
      AutoMigrate = envOr "NINJACAT_AUTO_MIGRATE" "true" <> "false"
      MetricsRetention = metricsRetention ()
      TrustedProxies =
        (envOr "NINJACAT_TRUSTED_PROXIES" "").Split(',', StringSplitOptions.RemoveEmptyEntries ||| StringSplitOptions.TrimEntries)
        |> List.ofArray
      // Agents send a few megabytes; a flare or a profile is tens of them.
      MaxBodyBytes =
        match Int64.TryParse(envOr "NINJACAT_MAX_BODY_BYTES" "") with
        | true, bytes when bytes > 0L -> bytes
        | _ -> 64L * 1024L * 1024L
      AckUnknown = envOr "NINJACAT_ACK_UNKNOWN" "" = "true"
      Debug = envOr "DEBUG" "" = "true"
      CaptureDir = envOr "NINJACAT_CAPTURE_DIR" "captures"
      SelfMonitorInterval = duration "NINJACAT_SELFMON_INTERVAL" (TimeSpan.FromSeconds 15.0)
      SelfMonitorTenant = envOr "NINJACAT_SELFMON_TENANT" "default"
      SelfMonitorHost = envOr "NINJACAT_SELFMON_HOST" "" }
