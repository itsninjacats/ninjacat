/// The ClickHouse schema: numbered SQL files embedded in the binary, applied
/// in order and recorded in `schema_migrations`.
///
/// Applied migrations are immutable. Each is recorded with a checksum of its
/// file, and a file that changed after it was applied stops the start-up
/// rather than letting the schema drift from the files.
module NinjaCat.Api.Storage.Migrations

open System
open System.IO
open System.Reflection
open System.Text
open System.Text.RegularExpressions
open System.Threading.Tasks
open ClickHouse.Driver
open ClickHouse.Driver.ADO.Parameters
open ClickHouse.Driver.Utility

type Migration =
    { Version: uint32
      Name: string
      Sql: string
      Checksum: uint64 }

let private createLedger =
    """
CREATE TABLE IF NOT EXISTS schema_migrations (
    version    UInt32,
    name       String,
    applied_at DateTime64(3, 'UTC') DEFAULT now64(3),
    checksum   UInt64
) ENGINE = ReplacingMergeTree(applied_at) ORDER BY version"""

/// A migration file's checksum. The ledger already holds checksums the Go
/// server wrote with this function, so it cannot change.
let checksum (data: byte[]) : uint64 = Fnv.hash64 data

let parseFilename (name: string) : Result<uint32 * string, string> =
    let m = Regex.Match(name, @"^(\d{4})_([a-z0-9][a-z0-9_]*)\.sql$")

    if not m.Success then
        Error $"schema: migration filename \"{name}\" does not match NNNN_snake_name.sql"
    else
        match UInt32.Parse m.Groups[1].Value with
        | 0u -> Error $"schema: migration filename \"{name}\": version 0 is not a valid version"
        | version -> Ok(version, m.Groups[2].Value)

/// Splits a migration file into statements at each `;` outside quotes and
/// comments. Comments are dropped: ClickHouse over HTTP takes one statement
/// per request and refuses a request that is only a comment.
let splitStatements (sql: string) : string list =
    let statements = ResizeArray<string>()
    let current = StringBuilder()

    let flush () =
        let statement = current.ToString().Trim()
        if statement <> "" then statements.Add statement
        current.Clear() |> ignore

    let mutable i = 0
    let n = sql.Length

    while i < n do
        let c = sql[i]

        if c = '\'' || c = '`' || c = '"' then
            // A quoted run, copied whole; a backslash keeps the next character.
            current.Append c |> ignore
            i <- i + 1
            let mutable closed = false

            while i < n && not closed do
                if sql[i] = '\\' && i + 1 < n then
                    current.Append(sql[i]).Append(sql[i + 1]) |> ignore
                    i <- i + 2
                else
                    current.Append sql[i] |> ignore
                    closed <- sql[i] = c
                    i <- i + 1
        elif c = '-' && i + 1 < n && sql[i + 1] = '-' then
            while i < n && sql[i] <> '\n' do
                i <- i + 1
        elif c = '/' && i + 1 < n && sql[i + 1] = '*' then
            i <- i + 2
            let mutable closed = false

            while i < n && not closed do
                if sql[i] = '*' && i + 1 < n && sql[i + 1] = '/' then
                    i <- i + 2
                    closed <- true
                else
                    i <- i + 1

            current.Append ' ' |> ignore
        elif c = ';' then
            flush ()
            i <- i + 1
        else
            current.Append c |> ignore
            i <- i + 1

    flush ()
    List.ofSeq statements

/// Every embedded migration, by version.
let load () : Result<Migration list, string> =
    let assembly = Assembly.GetExecutingAssembly()
    let prefix = "migrations/"

    let read (resource: string) : Result<Migration, string> =
        match parseFilename (resource.Substring prefix.Length) with
        | Error e -> Error e
        | Ok(version, name) ->
            use stream = assembly.GetManifestResourceStream resource
            use buffer = new MemoryStream()
            stream.CopyTo buffer
            let data = buffer.ToArray()

            Ok
                { Version = version
                  Name = name
                  Sql = Encoding.UTF8.GetString data
                  Checksum = checksum data }

    let results =
        assembly.GetManifestResourceNames()
        |> Array.filter (fun r -> r.StartsWith prefix)
        |> Array.map read
        |> List.ofArray

    let firstError =
        results
        |> List.tryPick (fun r ->
            match r with
            | Error e -> Some e
            | Ok _ -> None)

    match firstError with
    | Some e -> Error e
    | None ->
        let migrations = results |> List.choose Result.toOption |> List.sortBy _.Version

        let duplicate =
            migrations
            |> List.pairwise
            |> List.tryFind (fun (a, b) -> a.Version = b.Version)

        match duplicate with
        | Some(a, b) -> Error $"schema: two migrations share version %04d{a.Version} ({a.Name} and {b.Name})"
        | None -> Ok migrations

/// The migrations still to apply, given what the ledger says was applied.
let plan (migrations: Migration list) (applied: Map<uint32, uint64>) : Result<Migration list, string> =
    let changed =
        migrations
        |> List.tryFind (fun m ->
            match applied.TryFind m.Version with
            | Some recorded -> recorded <> m.Checksum
            | None -> false)

    match changed with
    | Some m ->
        Error(
            $"schema: migration %04d{m.Version}_{m.Name} has already been applied but its file has changed "
            + $"(ledger checksum {applied[m.Version]}, file checksum {m.Checksum}). Applied migrations are immutable: "
            + "the edited SQL will never run here and the schema would silently diverge "
            + "from the files. Revert the edit and add a new migration instead"
        )
    | None -> Ok(migrations |> List.filter (fun m -> not (applied.ContainsKey m.Version)))

/// Applies what is pending; returns how many migrations ran.
let apply (client: ClickHouseClient) : Task<Result<int, string>> =
    task {
        match load () with
        | Error e -> return Error e
        | Ok migrations ->
            let! _ = client.ExecuteNonQueryAsync createLedger
            let mutable applied = Map.empty

            do!
                task {
                    use! reader = client.ExecuteReaderAsync "SELECT version, checksum FROM schema_migrations FINAL"

                    while reader.Read() do
                        applied <- applied.Add(reader.GetFieldValue<uint32> 0, reader.GetFieldValue<uint64> 1)
                }

            match plan migrations applied with
            | Error e -> return Error e
            | Ok pending ->
                let mutable failure = None
                let mutable complete = 0

                for m in pending do
                    if failure.IsNone then
                        try
                            for statement in splitStatements m.Sql do
                                let! _ = client.ExecuteNonQueryAsync statement
                                ()

                            let parameters = ClickHouseParameterCollection()
                            parameters.AddParameter("version", m.Version) |> ignore
                            parameters.AddParameter("name", m.Name) |> ignore
                            parameters.AddParameter("checksum", m.Checksum) |> ignore

                            let! _ =
                                client.ExecuteNonQueryAsync(
                                    "INSERT INTO schema_migrations (version, name, checksum) VALUES ({version:UInt32}, {name:String}, {checksum:UInt64})",
                                    parameters
                                )

                            complete <- complete + 1
                        with e ->
                            failure <- Some $"schema: migration %04d{m.Version}_{m.Name}: {e.Message}"

                match failure with
                | Some e -> return Error e
                | None -> return Ok complete
    }
