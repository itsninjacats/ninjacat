namespace NinjaCat.Api.Server

open System
open System.Threading
open System.Threading.Tasks
open Microsoft.Extensions.Hosting
open Microsoft.Extensions.Logging
open Npgsql
open NinjaCat.Api.Intake

/// Keeps the intake's API keys in step with Postgres: reads them all at
/// start-up, then every 30 seconds, and at once when the panel says a key
/// was added or removed.
///
/// The panel says so through Postgres (NOTIFY on `channel`), the one thing
/// both already share: the intake needs no port for the panel to call.
///
/// Postgres holds only the SHA-256 of each key, which is what the intake
/// looks up by.
type ApiKeysKeeper(db: NpgsqlDataSource, store: ApiKeys.Store, log: ILogger<ApiKeysKeeper>) =
    inherit BackgroundService()

    /// The same name is in the panel (frontend/src/lib/server/api-keys.ts).
    let channel = "ninjacat_api_keys"
    let refreshInterval = TimeSpan.FromSeconds 30.0

    let fetch (ct: CancellationToken) : Task =
        task {
            use command = db.CreateCommand "SELECT id::text, name, tenant_id, key_hash FROM api_key"
            use! reader = command.ExecuteReaderAsync ct
            let keys = ResizeArray<string * ApiKeys.Key>()
            let mutable more = true

            while more do
                let! next = reader.ReadAsync ct

                if next then
                    keys.Add(
                        reader.GetString 3,
                        { ID = reader.GetString 0
                          Name = reader.GetString 1
                          TenantID = reader.GetString 2 }
                    )
                else
                    more <- false

            store.Publish keys
        }

    /// Re-reads the keys at every notification and every interval, until
    /// the connection breaks. LISTEN belongs to a session, so it holds one.
    let listenAndRefresh (ct: CancellationToken) : Task =
        task {
            use! listener = db.OpenConnectionAsync ct
            use listen = new NpgsqlCommand($"LISTEN {channel}", listener)
            let! _ = listen.ExecuteNonQueryAsync ct

            while not ct.IsCancellationRequested do
                let! _ = listener.WaitAsync(refreshInterval, ct)
                do! fetch ct
        }

    /// Reads the keys once, now. Called before the intake accepts anything:
    /// with no keys every agent would be answered 403, which tells it to stop.
    member _.FetchNow() : unit =
        (fetch CancellationToken.None).GetAwaiter().GetResult()
        log.LogInformation("keeper started, {Keys} keys, refreshing every {Interval}", store.Current.ByHash.Count, refreshInterval)

    override _.ExecuteAsync(ct: CancellationToken) : Task =
        task {
            while not ct.IsCancellationRequested do
                try
                    do! listenAndRefresh ct
                with
                | :? OperationCanceledException -> ()
                | e ->
                    // The old keys stay in force. A Postgres that is down is
                    // asked again after the interval, not in a tight loop.
                    log.LogWarning("keeper: refresh failed: {Error}", e.Message)

                    try
                        do! Task.Delay(refreshInterval, ct)
                    with :? OperationCanceledException ->
                        ()
        }
