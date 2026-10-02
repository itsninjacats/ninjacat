/// Postgres: users, API keys, and later monitors and dashboards.
///
/// The schema belongs to Drizzle in `frontend/` — this service never migrates
/// it. Anything the F# side needs to store gets its table there first.
module NinjaCat.Api.Server.Postgres

open System
open System.Threading
open Npgsql

/// Turns the `postgres://user:pass@host:port/db?sslmode=...` URL the panel
/// uses too into Npgsql's key=value form, which is all Npgsql accepts.
let connectionString (url: string) =
    let uri = Uri url

    if uri.Scheme <> "postgres" && uri.Scheme <> "postgresql" then
        invalidArg (nameof url) $"DATABASE_URL must be a postgres:// URL, got scheme '{uri.Scheme}'"

    let user, password =
        match uri.UserInfo.Split(':', 2) with
        | [| u; p |] -> Uri.UnescapeDataString u, Uri.UnescapeDataString p
        | [| u |] -> Uri.UnescapeDataString u, ""
        | _ -> "", ""

    let b =
        NpgsqlConnectionStringBuilder(
            Host = uri.Host,
            Port = (if uri.IsDefaultPort || uri.Port < 0 then 5432 else uri.Port),
            Username = user,
            Password = password,
            Database = Uri.UnescapeDataString(uri.AbsolutePath.TrimStart '/')
        )

    // libpq's sslmode names match Npgsql's SslMode enum, case aside.
    let query = Web.HttpUtility.ParseQueryString uri.Query

    match query["sslmode"] with
    | null -> ()
    | mode -> b.SslMode <- Enum.Parse<SslMode>(mode.Replace("-", ""), ignoreCase = true)

    b.ConnectionString

let ping (db: NpgsqlDataSource) (ct: CancellationToken) =
    task {
        try
            use cmd = db.CreateCommand "SELECT 1"
            let! _ = cmd.ExecuteScalarAsync ct
            return true
        with
        | :? NpgsqlException -> return false
        | :? TimeoutException -> return false
    }
