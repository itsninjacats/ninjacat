module NinjaCat.Api.Server.Config

open System

let private envOr (key: string) (fallback: string) =
    match Environment.GetEnvironmentVariable key with
    | null
    | "" -> fallback
    | v -> v

type Config =
    { ListenUrl: string
      ClickHouseHttpAddr: string
      ClickHouseDb: string
      ClickHouseUser: string
      ClickHousePassword: string
      DatabaseUrl: string }

// The CLICKHOUSE_* names match the Go server's so one .env serves both. The
// address is the exception: Go speaks the native protocol on :9000, the .NET
// driver speaks HTTP on :8123, so they cannot share CLICKHOUSE_ADDR.
let load () =
    { ListenUrl = envOr "NINJACAT_API_URL" "http://localhost:8082"
      ClickHouseHttpAddr = envOr "CLICKHOUSE_HTTP_ADDR" "localhost:8123"
      ClickHouseDb = envOr "CLICKHOUSE_DB" "ninjacat"
      ClickHouseUser = envOr "CLICKHOUSE_USER" "ninjacat"
      ClickHousePassword = envOr "CLICKHOUSE_PASSWORD" "ninjacat"
      // Required, as in the Go keeper: there is no sensible default database.
      DatabaseUrl =
        match envOr "DATABASE_URL" "" with
        | "" -> failwith "DATABASE_URL is not set (e.g. postgres://root:mysecretpassword@localhost:5433/local)"
        | url -> url }
