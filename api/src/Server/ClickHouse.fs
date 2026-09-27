/// Executes what the engine compiled. The only file that knows the driver.
module NinjaCat.Api.Server.ClickHouse

open System
open System.Threading
open System.Threading.Tasks
open ClickHouse.Driver
open ClickHouse.Driver.ADO
open ClickHouse.Driver.Utility
open NinjaCat.Api.Engine

/// Server-side limit for a single query, in seconds.
///
/// Mirrors the Go query worker: a slow query fails in ClickHouse with a
/// readable message, before the HTTP client gives up with an opaque timeout.
let private maxExecutionSeconds = 7

let createClient (cfg: Config.Config) =
    let host, port =
        match cfg.ClickHouseHttpAddr.Split ':' with
        | [| h; p |] -> h, UInt16.Parse p
        | _ -> cfg.ClickHouseHttpAddr, 8123us

    let settings =
        ClickHouseClientSettings(
            Host = host,
            Port = port,
            Database = cfg.ClickHouseDb,
            Username = cfg.ClickHouseUser,
            Password = cfg.ClickHousePassword,
            Timeout = TimeSpan.FromSeconds(float maxExecutionSeconds + 1.0)
        )

    // This service only reads — the Go server owns the schema and every
    // write. readonly=2 makes ClickHouse enforce that per request while still
    // letting the driver set its own session settings (readonly=1 would not).
    // A dedicated read-only ClickHouse user is the stronger form of the same
    // rule; this holds until one exists.
    settings.CustomSettings["readonly"] <- box 2
    settings.CustomSettings["max_execution_time"] <- box maxExecutionSeconds
    new ClickHouseClient(settings)

let private toDriverValue =
    function
    | SqlValue.String s -> box s
    | SqlValue.Int64 i -> box i
    | SqlValue.Float64 f -> box f
    | SqlValue.StringArray xs -> box (List.toArray xs)

/// Runs a query and maps each row with `read`.
let query (client: ClickHouseClient) (ct: CancellationToken) (read: Data.Common.DbDataReader -> 'T) (sql: Sql) : Task<'T list> =
    task {
        use conn = client.CreateConnection()
        use cmd = conn.CreateCommand(sql.Text)

        // The driver reads each parameter's type from its `{name:Type}`
        // placeholder, which the engine derived from the same SqlValue.
        for name, value in sql.Parameters do
            cmd.AddParameter(name, toDriverValue value) |> ignore

        use! reader = cmd.ExecuteReaderAsync(ct)
        let rows = ResizeArray()

        let mutable more = true
        while more do
            let! next = reader.ReadAsync(ct)
            if next then rows.Add(read reader) else more <- false

        return List.ofSeq rows
    }

let ping (client: ClickHouseClient) (ct: CancellationToken) = client.PingAsync(null, ct)

/// The engine's `execute` for metric queries: rows in the shape Compile
/// promises — N group columns, then bucket_ms, then value.
let metricRows (client: ClickHouseClient) (ct: CancellationToken) (sql: Sql) : Task<NinjaCat.Api.Engine.Query.Compile.Row list> =
    sql
    |> query client ct (fun r ->
        let groups = r.FieldCount - 2

        { Groups = [ for i in 0 .. groups - 1 -> if r.IsDBNull i then None else Some(r.GetString i) ]
          BucketMs = r.GetInt64 groups
          Value = r.GetDouble(groups + 1) }
        : NinjaCat.Api.Engine.Query.Compile.Row)
