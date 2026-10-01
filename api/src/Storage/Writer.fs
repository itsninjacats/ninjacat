namespace NinjaCat.Api.Storage

open System
open System.Threading
open System.Threading.Tasks
open Microsoft.Extensions.Logging

type WriterStats =
    { Table: string
      Buffered: int
      Written: int64
      Dropped: int64
      Failed: int64
      InFlight: int }

type WriterLimits =
    { MaxRows: int
      FlushInterval: TimeSpan
      BufferLimit: int
      MaxInFlight: int }

/// Buffers rows for ONE table and inserts them in batches.
///
/// One writer per table, never a pool for the same table: a pool would split
/// the buffer into smaller batches and bring back ClickHouse's "Too many
/// parts". A slow insert does not block the next one either — the full buffer
/// is handed to a background task and a fresh one takes its place.
type TableWriter(table: string, limits: WriterLimits, insert: obj[][] -> Task, log: ILogger) =
    let gate = obj ()
    let mutable buffer = ResizeArray<obj[]>(limits.MaxRows)
    let mutable inFlight = 0
    let mutable written = 0L
    let mutable dropped = 0L
    let mutable failed = 0L

    let send (batch: obj[][]) : Task =
        task {
            try
                do! insert batch
                Interlocked.Add(&written, int64 batch.Length) |> ignore
            with e ->
                // The batch is lost on purpose: putting it back would need the
                // ordering and memory guarantees this design avoids.
                Interlocked.Add(&failed, int64 batch.Length) |> ignore
                log.LogError(e, "writer {Table}: insert of {Rows} rows failed", table, batch.Length)
        }

    /// Takes the buffer if a flush may start now. Call under the lock.
    let takeBatch () : obj[][] option =
        if buffer.Count = 0 then
            None
        elif inFlight >= limits.MaxInFlight then
            log.LogWarning("writer {Table}: {InFlight} flushes in flight, deferring ({Buffered} buffered)", table, inFlight, buffer.Count)
            None
        else
            let batch = buffer.ToArray()
            buffer <- ResizeArray<obj[]>(limits.MaxRows)
            inFlight <- inFlight + 1
            Some batch

    /// Flushes still running, so that stopping can wait for them.
    let running = System.Collections.Generic.HashSet<Task>()

    let flushInBackground (batch: obj[][]) =
        let work =
            Task.Run(fun () ->
                task {
                    try
                        do! send batch
                    finally
                        lock gate (fun () -> inFlight <- inFlight - 1)
                }
                :> Task)

        lock gate (fun () -> running.Add work |> ignore)
        work.ContinueWith(fun (finished: Task) -> lock gate (fun () -> running.Remove finished |> ignore)) |> ignore

    let flush () =
        match lock gate takeBatch with
        | Some batch -> flushInBackground batch
        | None -> ()

    let timer = new Timer((fun _ -> flush ()), null, limits.FlushInterval, limits.FlushInterval)

    member _.Add(rows: obj[][]) =
        let batch =
            lock gate (fun () ->
                if buffer.Count + rows.Length > limits.BufferLimit then
                    dropped <- dropped + int64 rows.Length
                    log.LogWarning("writer {Table}: buffer full ({Buffered} rows), dropped {Rows} (total {Dropped})", table, buffer.Count, rows.Length, dropped)
                    None
                else
                    buffer.AddRange rows
                    if buffer.Count >= limits.MaxRows then takeBatch () else None)

        match batch with
        | Some batch -> flushInBackground batch
        | None -> ()

    member _.Stats: WriterStats =
        lock gate (fun () ->
            { Table = table
              Buffered = buffer.Count
              Written = Interlocked.Read &written
              Dropped = dropped
              Failed = Interlocked.Read &failed
              InFlight = inFlight })

    /// Stops the timer, waits for the flushes under way and inserts what is
    /// left: at shutdown there is nothing left to protect from a slow insert.
    member _.StopAsync() : Task =
        task {
            do! timer.DisposeAsync()
            let underWay = lock gate (fun () -> Array.ofSeq running)
            do! Task.WhenAll underWay

            let rest =
                lock gate (fun () ->
                    let batch = buffer.ToArray()
                    buffer <- ResizeArray<obj[]>()
                    batch)

            if rest.Length > 0 then
                do! send rest
        }
