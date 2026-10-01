namespace NinjaCat.Api.Storage

open System
open System.Collections.Concurrent
open System.Collections.Generic
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
    let columnTypes (table: string) : Task<IReadOnlyDictionary<string, string>> =
        task {
            let types = Dictionary<string, string>()
            let parameters = ClickHouseParameterCollection()
            parameters.AddParameter("table", table) |> ignore

            use! reader =
                client.ExecuteReaderAsync(
                    "SELECT name, type FROM system.columns WHERE database = currentDatabase() AND table = {table:String}",
                    parameters
                )

            while reader.Read() do
                types[reader.GetString 0] <- (reader.GetString 1).Replace("DateTime64(9, 'UTC')", "Int64")

            return types :> IReadOnlyDictionary<string, string>
        }

    let rec nanosAsInt64 (value: obj) : obj =
        match value with
        | :? UnixNanos as n ->
            let (UnixNanos ns) = n
            box ns
        | :? (UnixNanos[]) as many -> many |> Array.map (fun (UnixNanos ns) -> ns) |> box
        | other -> other

    let createWriter (name: string) (columns: string list) (limits: WriterLimits) : TableWriter =
        let mutable options: InsertOptions = null

        let insert (rows: obj[][]) : Task =
            task {
                if isNull options then
                    let! types = columnTypes name
                    options <- InsertOptions(ColumnTypes = types)

                for row in rows do
                    for i in 0 .. row.Length - 1 do
                        row[i] <- nanosAsInt64 row[i]

                let! _ = client.InsertBinaryAsync(name, columns, rows, options)
                return ()
            }

        TableWriter(name, limits, insert, log)

    member _.Stats: WriterStats list = writers.Values |> Seq.map _.Stats |> Seq.sortBy _.Table |> List.ofSeq

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
