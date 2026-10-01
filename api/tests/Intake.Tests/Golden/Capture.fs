namespace NinjaCat.Api.Intake.Tests.Golden

open NinjaCat.Api.Storage

/// One `Sink.write`: the table, the rows as records, and each row as the
/// column values that would be inserted.
type CapturedWrite =
    { Writer: string
      Table: string
      Columns: string list
      Rows: obj[]
      Args: obj[][] }

/// A sink that remembers instead of writing.
type CapturingSink() =
    let writes = ResizeArray<CapturedWrite>()

    member _.Writes: CapturedWrite list = List.ofSeq writes

    /// Every captured row of one row type, across all writes.
    member _.Rows<'row>() : 'row list =
        writes |> Seq.collect _.Rows |> Seq.choose (fun row -> match row with :? 'row as r -> Some r | _ -> None) |> List.ofSeq

    interface ISink with
        member _.Write(table: Table<'row>, rows: 'row[]) =
            writes.Add
                { Writer = table.Writer
                  Table = table.Name
                  Columns = table.Columns
                  Rows = rows |> Array.map box
                  Args = rows |> Array.map table.Values }
