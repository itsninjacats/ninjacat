module NinjaCat.Api.Intake.Tests.MigrationsTests

open System.Text
open Xunit
open NinjaCat.Api.Storage

// --- splitting a file into statements ---------------------------------------------------

[<Fact>]
let ``the real initial migration splits into its 17 CREATE statements`` () =
    let initial =
        match Migrations.load () with
        | Ok migrations -> migrations |> List.find (fun m -> m.Version = 1u)
        | Error e -> failwith e

    let statements = Migrations.splitStatements initial.Sql
    Assert.Equal(17, statements.Length)

    for statement in statements do
        Assert.StartsWith("CREATE ", statement)
        Assert.DoesNotContain(";", statement)

    let joined = String.concat "\n" statements

    // Quotes, brackets and commas inside expressions survive.
    for kept in
        [ "tags['env'][1]"
          "Enum8('OK' = 0, 'WARNING' = 1, 'CRITICAL' = 2, 'UNKNOWN' = 3)"
          "sipHash64(tenant_id, metric, host, tags)"
          "ReplacingMergeTree(collected_at)" ] do
        Assert.Contains(kept, joined)

[<Fact>]
let ``a semicolon inside a string or a comment does not split`` () =
    Assert.Equal<string list>([ "SELECT 'a;b'"; "SELECT 2" ], Migrations.splitStatements "SELECT 'a;b'; SELECT 2")

    Assert.Equal<string list>(
        [ "SELECT 1"; "SELECT 2" ],
        Migrations.splitStatements "-- naglowek; z przecinkiem, i srednikiem\nSELECT 1; -- ogon; tez\nSELECT 2"
    )

    Assert.Equal<string list>([ "SELECT 'a--b;c'" ], Migrations.splitStatements "SELECT 'a--b;c'")

[<Fact>]
let ``escaped and doubled quotes stay inside their string`` () =
    Assert.Equal<string list>([ @"SELECT 'it\'s;fine'" ], Migrations.splitStatements @"SELECT 'it\'s;fine'")
    Assert.Equal<string list>([ "SELECT 'it''s;fine'" ], Migrations.splitStatements "SELECT 'it''s;fine'")

[<Fact>]
let ``block comments become a space, empty fragments vanish`` () =
    Assert.Equal<string list>([ "SELECT 1" ], Migrations.splitStatements "/* only; a comment */ ;;\n  ;\nSELECT/*x*/1;")

// --- file names, order, the ledger -----------------------------------------------------

[<Fact>]
let ``a migration file is NNNN_snake_name.sql`` () =
    Assert.Equal(Ok(1u, "initial"), Migrations.parseFilename "0001_initial.sql")
    Assert.Equal(Ok(42u, "add_traces_table"), Migrations.parseFilename "0042_add_traces_table.sql")

    for bad in
        [ "initial.sql"
          "1_initial.sql"
          "00001_initial.sql"
          "0001-initial.sql"
          "0001_Initial.sql"
          "0001_initial.SQL"
          "0001_.sql"
          "0000_reserved.sql"
          "0001_initial.sql.bak" ] do
        Assert.True(Result.isError (Migrations.parseFilename bad), bad)

[<Fact>]
let ``the embedded migrations load in version order, each with a checksum`` () =
    match Migrations.load () with
    | Error e -> Assert.Fail e
    | Ok migrations ->
        Assert.Equal(1u, migrations.Head.Version)
        Assert.Equal<uint32 list>(migrations |> List.map _.Version |> List.sort |> List.distinct, migrations |> List.map _.Version)
        Assert.All(migrations, (fun m -> Assert.NotEqual(0UL, m.Checksum)))

let private migration (version: uint32) (name: string) (checksum: uint64) : Migrations.Migration =
    { Version = version; Name = name; Sql = ""; Checksum = checksum }

[<Fact>]
let ``what the ledger has is skipped, the rest is pending in order`` () =
    let migrations = [ migration 1u "one" 11UL; migration 2u "two" 22UL; migration 3u "three" 33UL ]
    let pending (applied: (uint32 * uint64) list) =
        match Migrations.plan migrations (Map applied) with
        | Ok pending -> pending |> List.map _.Version
        | Error e -> failwith e

    Assert.Equal<uint32 list>([ 1u; 2u; 3u ], pending [])
    Assert.Equal<uint32 list>([ 2u; 3u ], pending [ 1u, 11UL ])
    Assert.Equal<uint32 list>([], pending [ 1u, 11UL; 2u, 22UL; 3u, 33UL ])
    // A version only the ledger knows (a newer binary ran here) is tolerated.
    Assert.Equal<uint32 list>([], pending [ 1u, 11UL; 2u, 22UL; 3u, 33UL; 4u, 44UL ])

[<Fact>]
let ``an applied migration whose file changed stops everything, by name`` () =
    match Migrations.plan [ migration 1u "initial" 11UL ] (Map [ 1u, 999UL ]) with
    | Ok pending -> Assert.Fail $"planned %A{pending}"
    | Error e ->
        Assert.Contains("0001_initial", e)
        Assert.Contains("changed", e)

[<Fact>]
let ``the checksum is FNV-1a 64, the function the existing ledger was written with`` () =
    // Reference values of FNV-1a 64.
    Assert.Equal(0xcbf29ce484222325UL, Migrations.checksum [||])
    Assert.Equal(0xaf63dc4c8601ec8cUL, Migrations.checksum (Encoding.ASCII.GetBytes "a"))
    Assert.Equal(0xa430d84680aabd0bUL, Migrations.checksum (Encoding.ASCII.GetBytes "hello"))
