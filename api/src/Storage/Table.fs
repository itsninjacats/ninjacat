namespace NinjaCat.Api.Storage

open System

/// A point in time as Unix nanoseconds. DateTime stops at 100 ns; span and
/// CI event start times arrive, and are stored, at 1 ns.
[<Struct>]
type UnixNanos = UnixNanos of int64

/// One ClickHouse table as the write path sees it: where its rows go and how
/// a row becomes column values.
type Table<'row> =
    { /// The table's name on the write path ("storage_logs"). Kept from the
      /// Go server, where it named the writer process; tests and logs use it.
      Writer: string
      Name: string
      /// In the order `Values` returns them.
      Columns: string list
      Values: 'row -> obj[]
      /// Flush once this many rows are buffered…
      MaxRows: int
      /// …or this much time has passed.
      FlushInterval: TimeSpan
      /// Rows beyond this are dropped while ClickHouse is unreachable: losing
      /// data beats running out of memory.
      BufferLimit: int
      /// Concurrent flushes, so a slow database does not multiply connections.
      MaxInFlight: int }

module Table =
    let create (writer: string) (name: string) (columns: string list) (values: 'row -> obj[]) : Table<'row> =
        { Writer = writer
          Name = name
          Columns = columns
          Values = values
          MaxRows = 5000
          FlushInterval = TimeSpan.FromSeconds 2.0
          BufferLimit = 200_000
          MaxInFlight = 4 }

/// Where handlers put rows. Writing never waits: the agent gets its answer
/// first, and batching and flushing happen behind this.
type ISink =
    abstract Write<'row> : table: Table<'row> * rows: 'row[] -> unit

module Sink =
    let write (sink: ISink) (table: Table<'row>) (rows: 'row[]) =
        if rows.Length > 0 then
            sink.Write(table, rows)

/// Helpers for `Table.Values`: what the ClickHouse driver cannot take as is.
module Col =
    /// None is NULL.
    let opt (value: 'a option) : obj =
        match value with
        | Some v -> box v
        | None -> null

    /// A JSON column must hold an object; an empty string is not one.
    let jsonObject (text: string) : obj = if String.IsNullOrEmpty text then box "{}" else box text
