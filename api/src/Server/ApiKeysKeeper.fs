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
/// Postgres holds only the SHA-256 of each key, which is what the intake
/// looks up by.
type ApiKeysKeeper(db: NpgsqlDataSource, store: ApiKeys.Store, log: ILogger<ApiKeysKeeper>) =
    inherit BackgroundService()

    let refreshInterval = TimeSpan.FromSeconds 30.0
    let wake = new SemaphoreSlim(0, 1)

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

    /// Reads the keys again now, without waiting for the next round.
    member _.RequestRefresh() : unit =
        try
            wake.Release() |> ignore
        with :? SemaphoreFullException ->
            // A refresh is already pending; one is enough.
            ()

    /// Reads the keys once, now. Called before the intake accepts anything:
    /// with no keys every agent would be answered 403, which tells it to stop.
    member _.FetchNow() : unit =
        (fetch CancellationToken.None).GetAwaiter().GetResult()
        log.LogInformation("keeper started, {Keys} keys, refreshing every {Interval}", store.Current.ByHash.Count, refreshInterval)

    override _.ExecuteAsync(ct: CancellationToken) : Task =
        task {
            while not ct.IsCancellationRequested do
                try
                    let! _ = wake.WaitAsync(refreshInterval, ct)
                    do! fetch ct
                with
                | :? OperationCanceledException -> ()
                // The old keys stay in force; the next round tries again.
                | e -> log.LogWarning("keeper: refresh failed: {Error}", e.Message)
        }
