module NinjaCat.Api.Server.Config

open System
open NinjaCat.Api.Engine

let private envOr (key: string) (fallback: string) =
    match Environment.GetEnvironmentVariable key with
    | null
    | "" -> fallback
    | v -> v

/// `:8080` and `0.0.0.0:8080`, as the Go server took them, in the URL form
/// Kestrel wants.
let private listenUrl (address: string) : string =
    if address.Contains "://" then address
    elif address.StartsWith ":" then $"http://*{address}"
    else $"http://{address}"

/// A duration as Go wrote it ("15s"), or a bare number of seconds.
let private duration (key: string) (fallback: TimeSpan) : TimeSpan =
    let raw = envOr key ""

    match Panel.parseDuration raw, Int32.TryParse raw with
    | Ok d, _ when d > TimeSpan.Zero -> d
    | _, (true, seconds) when seconds > 0 -> TimeSpan.FromSeconds(float seconds)
    | _ -> fallback

type Config =
    { /// Where the intake process listens (NINJACAT_ADDR). Reachable from
      /// agent machines.
      IntakeUrl: string
      /// Where the query process listens (NINJACAT_INTERNAL_ADDR): the panel's
      /// and the query API. Internal only: nothing on it asks for a key.
      InternalUrl: string
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
      /// Answer 202 on unknown intake paths (NINJACAT_ACK_UNKNOWN).
      AckUnknown: bool
      /// DEBUG=true: every intake request is also dumped to CaptureDir.
      Debug: bool
      CaptureDir: string
      SelfMonitorInterval: TimeSpan
      SelfMonitorTenant: string
      SelfMonitorHost: string }

// The CLICKHOUSE_* names are the ones the Go server used, so one .env serves
// both while they coexist. The address is the exception: Go spoke the native
// protocol on :9000, the .NET driver speaks HTTP on :8123.
let load () =
    { IntakeUrl = listenUrl (envOr "NINJACAT_ADDR" ":8080")
      InternalUrl = listenUrl (envOr "NINJACAT_INTERNAL_ADDR" ":8081")
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
      AckUnknown = envOr "NINJACAT_ACK_UNKNOWN" "" = "true"
      Debug = envOr "DEBUG" "" = "true"
      CaptureDir = envOr "NINJACAT_CAPTURE_DIR" "captures"
      SelfMonitorInterval = duration "NINJACAT_SELFMON_INTERVAL" (TimeSpan.FromSeconds 15.0)
      SelfMonitorTenant = envOr "NINJACAT_SELFMON_TENANT" "default"
      SelfMonitorHost = envOr "NINJACAT_SELFMON_HOST" "" }
