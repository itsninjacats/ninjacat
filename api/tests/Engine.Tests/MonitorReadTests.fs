module NinjaCat.Api.Engine.Tests.MonitorReadTests

open System
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Monitor

let private acme = TenantId "acme"
let private noon = DateTimeOffset(2026, 10, 14, 12, 0, 0, TimeSpan.Zero)

let private statements: (string * Sql) list =
    [ "status", Read.status acme [ 7L; 8L ]
      "groups", Read.groups acme 7L
      "transitions", Read.transitions acme 7L (noon.AddHours -1.0) noon "host:a"
      "initial", Read.initial acme 7L (noon.AddHours -1.0) "host:a"
      "evaluations", Read.evaluations acme 7L (noon.AddHours -1.0) noon "" ]

[<Fact>]
let ``every statement binds the tenant, in every table it reads`` () =
    for name, sql in statements do
        let tenants = sql.Parameters |> List.filter (fun (_, value) -> value = SqlValue.String "acme") |> List.length
        let tables = sql.Text.Split("FROM monitor_").Length - 1
        Assert.True(tables > 0, name)
        Assert.True((tenants = tables), $"{name}: {tables} tables read, the tenant bound {tenants} times")
        Assert.DoesNotContain("acme", sql.Text)

[<Fact>]
let ``a caller's group and ids are bound, never written into the SQL`` () =
    let transitions = Read.transitions acme 7L (noon.AddHours -1.0) noon "host:a' OR 1=1"
    Assert.DoesNotContain("OR 1=1", transitions.Text)
    Assert.Contains(SqlValue.StringArray [ "host:a' OR 1=1" ], transitions.Parameters |> List.map snd)
    let initial = Read.initial acme 7L noon "host:a' OR 1=1"
    Assert.DoesNotContain("OR 1=1", initial.Text)
    Assert.Contains(SqlValue.String "host:a' OR 1=1", initial.Parameters |> List.map snd)
    Assert.Contains(SqlValue.Int64Array [ 7L; 8L ], Read.status acme [ 7L; 8L ] |> _.Parameters |> List.map snd)

[<Fact>]
let ``transitions are read FINAL; without a group, every group`` () =
    Assert.Contains("FROM monitor_transitions FINAL", (snd statements[2]).Text)
    Assert.DoesNotContain("group_key IN", (Read.transitions acme 7L noon noon "").Text)
    Assert.Contains(SqlValue.StringArray [ "host:a" ], (Read.transitions acme 7L noon noon "host:a").Parameters |> List.map snd)

[<Fact>]
let ``one group's evaluations come with the monitor's own`` () =
    Assert.DoesNotContain("group_key IN", (Read.evaluations acme 7L noon noon "").Text)
    Assert.Contains(SqlValue.StringArray [ "host:a"; "" ], (Read.evaluations acme 7L noon noon "host:a").Parameters |> List.map snd)

[<Fact>]
let ``where a range starts is each group's latest transition, unless it closed the group`` () =
    let initial = Read.initial acme 7L noon ""
    Assert.Contains("LIMIT 1 BY group_key", initial.Text)
    Assert.Contains("reason NOT IN", initial.Text)
    Assert.Contains(SqlValue.StringArray [ "expired"; "definition_changed"; "deleted" ], initial.Parameters |> List.map snd)

[<Fact>]
let ``ids are comma-separated or repeated, positive, at most maxIds`` () =
    Assert.Equal(Ok [ 1L; 2L; 3L ], Read.parseIds [ "1, 2"; "3,1" ])
    Assert.Equal(Error "ids is required", Read.parseIds [])
    Assert.Equal(Error "ids is required", Read.parseIds [ "," ])
    Assert.Equal(Error "bad monitor id \"x\"", Read.parseIds [ "1,x" ])
    Assert.Equal(Error "bad monitor id \"-4\"", Read.parseIds [ "-4" ])
    Assert.True(Result.isOk (Read.parseIds [ String.Join(",", [ 1 .. Read.maxIds ]) ]))
    Assert.True(Result.isError (Read.parseIds [ String.Join(",", [ 1 .. Read.maxIds + 1 ]) ]))

let private row monitor state isOpen groups (triggered: int option) : Read.StatusRow =
    { Monitor = monitor
      State = state
      Open = isOpen
      Groups = groups
      LastTriggeredAt = triggered |> Option.map (fun m -> noon.AddMinutes(float -m)) }

[<Fact>]
let ``a monitor's state is its most severe open group's, one status per id in order`` () =
    let rows =
        [ row 7L "OK" true 3L None
          row 7L "No Data" true 1L (Some 30)
          row 7L "Warn" true 2L (Some 20)
          // Closed groups count only for when the monitor last triggered.
          row 7L "OK" false 4L (Some 5)
          row 8L "OK" false 1L None
          row 9L "Alert" true 1L (Some 1) ]

    match Read.statuses [ 8L; 7L; 10L ] rows with
    | [ eight; seven; ten ] ->
        Assert.Equal(Some "Warn", seven.State)
        Assert.Equal<Map<string, int64>>(Map [ "OK", 3L; "Warn", 2L; "Alert", 0L; "No Data", 1L ], seven.Groups)
        Assert.Equal(Some(noon.AddMinutes -5.0), seven.LastTriggeredAt)
        Assert.Equal(None, eight.State)
        Assert.Equal(0L, eight.Groups["OK"])
        Assert.Equal(10L, ten.Id)
        Assert.Equal(None, ten.State)
        Assert.Equal(None, ten.LastTriggeredAt)
    | other -> failwith $"{other}"

[<Fact>]
let ``past the row limit the oldest rows are dropped, and that is said`` () =
    let rows = [ 0 .. Read.rowLimit ]
    let kept, truncated = Read.newest rows
    Assert.True truncated
    Assert.Equal(Read.rowLimit, kept.Length)
    Assert.Equal(1, kept.Head)
    Assert.Equal(([ 1; 2 ], false), Read.newest [ 1; 2 ])
