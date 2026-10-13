namespace NinjaCat.Api.Storage

open System
open System.Collections.Concurrent
open System.Collections.Generic
open System.Threading
open System.Threading.Tasks
open ClickHouse.Driver
open ClickHouse.Driver.ADO.Parameters
open ClickHouse.Driver.Utility
open Microsoft.Extensions.Logging

/// The sink that writes to ClickHouse: one TableWriter per table, created the
/// first time a table is written to.
type ClickHouseSink(client: ClickHouseClient, log: ILogger) =
    let writers = ConcurrentDictionary<string, TableWriter>()

    /// The table's column types, with nanosecond timestamps declared as the
    /// Int64 they are on the wire. The driver would otherwise ask for a
    /// DateTime, which cannot hold nanoseconds; a UnixNanos is written as is.
    let columnTypes (table: string) (cancel: CancellationToken) : Task<IReadOnlyDictionary<string, string>> =
        task {
            let types = Dictionary<string, string>()
            let parameters = ClickHouseParameterCollection()
            parameters.AddParameter("table", table) |> ignore

            use! reader =
                client.ExecuteReaderAsync(
                    "SELECT name, type FROM system.columns WHERE database = currentDatabase() AND table = {table:String}",
                    parameters,
                    cancellationToken = cancel
                )

            while reader.Read() do
                types[reader.GetString 0] <- (reader.GetString 1).Replace("DateTime64(9, 'UTC')", "Int64")

            return types :> IReadOnlyDictionary<string, string>
        }

    /// What ClickHouse's own range allows for a column; a zero time
    /// (0001-01-01), which a row holds for "none", is the epoch. One value
    /// outside the range would otherwise fail the whole batch it travels in.
    let clamp (earliest: DateTime) (latest: DateTime) (time: DateTime) : DateTime =
        if time = DateTime.MinValue then DateTime.UnixEpoch
        elif time < earliest then earliest
        elif time > latest then latest
        else time

    /// How a column's values are adjusted before they are written, from the
    /// column's type.
    let adjusterFor (columnType: string) : obj -> obj =
        if columnType.Contains "Int64" then
            // Among these are the DateTime64(9) columns, declared Int64 by
            // `columnTypes`: a UnixNanos is written as the number it holds.
            fun value ->
                match value with
                | :? UnixNanos as n ->
                    let (UnixNanos ns) = n
                    box ns
                | :? (UnixNanos[]) as many -> many |> Array.map (fun (UnixNanos ns) -> ns) |> box
                | other -> other
        elif columnType.Contains "DateTime" then
            let inRange =
                if columnType.Contains "DateTime64" then
                    clamp (DateTime(1900, 1, 1, 0, 0, 0, DateTimeKind.Utc)) (DateTime(2262, 4, 11, 23, 47, 16, DateTimeKind.Utc))
                else
                    clamp DateTime.UnixEpoch (DateTime(2105, 12, 31, 23, 59, 59, DateTimeKind.Utc))

            fun value ->
                match value with
                | :? DateTime as time -> box (inRange time)
                | :? (DateTime[]) as times -> box (Array.map inRange times)
                | :? (Nullable<DateTime>[]) as times ->
                    times |> Array.map (fun time -> if time.HasValue then Nullable(inRange time.Value) else time) |> box
                | other -> other
        else
            id

    /// Inserts rows into one table, adjusted to its column types; the types
    /// are read the first time, and read again after a failed read.
    let inserter
        (name: string)
        (quotedColumns: string list)
        (settings: (string * obj) list)
        : CancellationToken -> obj[][] -> Task =
        // A column named like a keyword (`group`) is written with backticks
        // in a table's column list; the driver quotes names itself.
        let columns = quotedColumns |> List.map (fun column -> column.Trim '`')
        let mutable prepared: (InsertOptions * (obj -> obj)[]) option = None

        let prepare (cancel: CancellationToken) : Task<InsertOptions * (obj -> obj)[]> =
            task {
                match prepared with
                | Some ready -> return ready
                | None ->
                    let! types = columnTypes name cancel

                    let adjusterOf (column: string) =
                        match types.TryGetValue column with
                        | true, columnType -> adjusterFor columnType
                        | false, _ -> failwith $"{name} has no column {column}"

                    let options =
                        InsertOptions(ColumnTypes = types, CustomSettings = Dictionary(settings |> List.map KeyValuePair))

                    let ready = options, columns |> List.map adjusterOf |> Array.ofList
                    prepared <- Some ready
                    return ready
            }

        fun cancel rows ->
            task {
                let! options, adjusters = prepare cancel

                for row in rows do
                    for i in 0 .. row.Length - 1 do
                        row[i] <- adjusters[i] row[i]

                let! _ = client.InsertBinaryAsync(name, columns, rows, options, cancel)
                return ()
            }

    let createWriter (name: string) (quotedColumns: string list) (limits: WriterLimits) : TableWriter =
        TableWriter(name, limits, inserter name quotedColumns [] CancellationToken.None, log)

    let inserters = ConcurrentDictionary<string, CancellationToken -> obj[][] -> Task>()

    member _.Stats: WriterStats list = writers.Values |> Seq.map _.Stats |> Seq.sortBy _.Table |> List.ofSeq

    /// Inserts the rows now and fails if ClickHouse does not take them, for
    /// rows that must be stored: the buffered writer drops a failed batch.
    /// Inserting the same rows again is the caller's retry. Where async
    /// inserts are on, ClickHouse answers when the rows are stored only if
    /// told to wait, so this insert does not leave that to the server's
    /// profile.
    member _.InsertAsync(table: Table<'row>, rows: 'row[], cancel: CancellationToken) : Task =
        if rows.Length = 0 then
            Task.CompletedTask
        else
            let awaited _ =
                inserter table.Name table.Columns [ "wait_for_async_insert", box 1 ]

            let insert = inserters.GetOrAdd(table.Writer, Func<_, _>(awaited))
            insert cancel (Array.map table.Values rows)

    member _.StopAsync() : Task =
        task {
            for writer in writers.Values do
                do! writer.StopAsync()
        }

    interface ISink with
        member _.Write(table: Table<'row>, rows: 'row[]) =
            let writer =
                writers.GetOrAdd(
                    table.Writer,
                    fun _ ->
                        createWriter
                            table.Name
                            table.Columns
                            { MaxRows = table.MaxRows
                              FlushInterval = table.FlushInterval
                              BufferLimit = table.BufferLimit
                              MaxInFlight = table.MaxInFlight }
                )

            writer.Add(Array.map table.Values rows)
