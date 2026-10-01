/// Tests of Routers/Dbm.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.DbmTests

open System
open System.Text
open System.Text.Json
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

let private json (text: string) : JsonElement =
    use doc = JsonDocument.Parse text
    doc.RootElement.Clone()

let private envelope (event: string) : Dbm.Envelope =
    match Dbm.decodeEnvelope (json event) with
    | Ok envelope -> envelope
    | Error e -> failwith $"the event did not decode: {e}"

let private receivedAt = DateTime(2026, 9, 23, 12, 0, 0, DateTimeKind.Utc)

let private row (track: string) (event: string) : DBMEventRow =
    Dbm.toRow "test-tenant" track receivedAt event (envelope event)

/// Posts a body through the engine with the test key; returns the answer and
/// what was written.
let private post (path: string) (body: string) : Response * CapturingSink =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let http = DefaultHttpContext()
    http.Request.Method <- "POST"
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString path
    http.Request.Headers["Dd-Api-Key"] <- StringValues Replay.testKey
    Replay.byGoNames deps [ "routeDBM" ] http (Encoding.UTF8.GetBytes body), sink

[<Fact>]
let ``ddtags as one comma-joined string becomes a tag list, and the db object stays an extra key`` () =
    // The oracle check's FQTPayload.
    let e =
        envelope
            """{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl",
                "ddagentversion":"7.60.0","ddsource":"oracle","ddtags":"env:prod, team:db,,",
                "dbm_type":"fqt","db":{"instance":"orcl","query_signature":"abc","statement":"select 1"}}"""

    Assert.Equal<string list>([ "env:prod"; "team:db" ], e.Tags)
    Assert.Equal("db1", e.Host)
    Assert.Equal("db1/orcl", e.DatabaseInstance)
    Assert.Equal("fqt", e.DBMType)
    Assert.Equal("oracle", e.Source)
    Assert.Equal("7.60.0", e.AgentVersion)
    Assert.True(e.Extra.ContainsKey "db")
    Assert.False(e.Extra.ContainsKey "ddtags")
    Assert.Empty e.Undecoded

[<Theory>]
[<InlineData("""{"ddtags":["env:prod","team:db"],"collection_interval":10,"oracle_activity":[{},{}]}""")>]
[<InlineData("""{"tags":["env:prod","team:db"],"min_collection_interval":10,"oracle_rows":[{},{}]}""")>]
let ``tags as a list are read under either key, and the interval under either name`` (event: string) =
    let e = envelope event
    Assert.Equal<string list>([ "env:prod"; "team:db" ], e.Tags)
    Assert.Equal(Some 10.0, e.CollectionInterval)

[<Fact>]
let ``ddtags and tags sent together are both kept, ddtags first`` () =
    let e = envelope """{"tags":["b:2"],"ddtags":"a:1"}"""
    Assert.Equal<string list>([ "a:1"; "b:2" ], e.Tags)

[<Fact>]
let ``agent_version stands in for ddagentversion`` () =
    // The oracle check's dbInstanceEvent says agent_version; everything else ddagentversion.
    let e = envelope """{"agent_version":"7.60.0","kind":"database_instance","dbms":"oracle"}"""
    Assert.Equal("7.60.0", e.AgentVersion)
    Assert.Equal("database_instance", e.Kind)
    Assert.Equal("oracle", e.DBMS)

[<Fact>]
let ``a malformed envelope field is noted and kept, and does not take the others down`` () =
    let e = envelope """{"host":"db1","timestamp":"not-a-number","postgres_rows":[1,2,3],"custom":{"a":1}}"""
    Assert.Equal("db1", e.Host)
    Assert.Equal<string list>([ "timestamp" ], e.Undecoded)
    Assert.Equal<string list>([ "custom"; "postgres_rows"; "timestamp" ], e.Extra.Keys |> Seq.sort |> List.ofSeq)
    Assert.Equal("\"not-a-number\"", e.Extra["timestamp"].GetRawText())

[<Fact>]
let ``null leaves a field empty without marking it undecoded`` () =
    let e = envelope """{"host":null,"timestamp":null,"collection_interval":null,"ddtags":null,"tags":["a:1",null]}"""
    Assert.Equal("", e.Host)
    Assert.Equal("", e.Timestamp)
    Assert.Equal(None, e.CollectionInterval)
    Assert.Equal<string list>([ "a:1"; "" ], e.Tags)
    Assert.Empty e.Undecoded
    Assert.True e.Extra.IsEmpty

[<Fact>]
let ``a malformed interval reads as zero, not as absent, and is noted`` () =
    // What Go's decoder did: it allocated the *float64 before refusing the value.
    let text = envelope """{"collection_interval":"x","min_collection_interval":3}"""
    Assert.Equal(Some 0.0, text.CollectionInterval)
    Assert.Equal<string list>([ "collection_interval" ], text.Undecoded)
    Assert.True(text.Extra.ContainsKey "collection_interval")

    Assert.Equal(Some 0.0, (envelope """{"min_collection_interval":true}""").CollectionInterval)
    Assert.Equal(Some 5.0, (envelope """{"collection_interval":5,"min_collection_interval":"x"}""").CollectionInterval)
    Assert.Equal(Some Double.PositiveInfinity, (envelope """{"collection_interval":1e999}""").CollectionInterval)
    Assert.Equal(Some 7.0, (envelope """{"collection_interval":null,"min_collection_interval":7}""").CollectionInterval)

[<Fact>]
let ``envelope keys match exactly, and the last of two equal keys wins`` () =
    let e = envelope """{"Host":"upper","host":"a","host":"b"}"""
    Assert.Equal("b", e.Host)
    Assert.Equal<string list>([ "Host" ], e.Extra.Keys |> List.ofSeq)

[<Fact>]
let ``the timestamp is kept as written, a number inside a string included`` () =
    Assert.Equal("1758326400123.456", (envelope """{"timestamp":1758326400123.456}""").Timestamp)
    Assert.Equal("1758326400123", (envelope """{"timestamp":"1758326400123"}""").Timestamp)
    // Absent means not sent, not 1970.
    Assert.Equal("", (envelope """{"host":"db1"}""").Timestamp)

[<Fact>]
let ``an event that is not an object is refused, null by name`` () =
    Assert.Equal(Error "event is null, not a JSON object", Dbm.decodeEnvelope (json "null") |> Result.map ignore)
    Assert.True((Dbm.decodeEnvelope (json "\"text\"")).IsError)
    Assert.True((Dbm.decodeEnvelope (json "42")).IsError)

[<Fact>]
let ``the agent's empty probe stores nothing`` () =
    for body in [ "{}"; "[]"; "" ] do
        let response, sink = post "/api/v2/dbmmetrics" body
        Assert.Equal(202, response.Status)
        Assert.Empty sink.Writes

[<Fact>]
let ``a body is an array of events, one bare object, or refused`` () =
    let count (body: string) =
        Dbm.splitEvents (Encoding.UTF8.GetBytes body) |> Result.map List.length

    Assert.Equal(Ok 2, count """[{"a":1},{"b":2}]""")
    Assert.Equal(Ok 1, count """{"a":1}""")
    Assert.Equal(Ok 0, count "null")
    Assert.True((count "not json").IsError)
    Assert.True((count "\"text\"").IsError)

[<Fact>]
let ``timestamp: an integer stays exact, a fraction rounds to the millisecond, absent is None`` () =
    Assert.Equal(Some(Time.fromUnixMillis 1758326400000L), Dbm.timestamp "1758326400000")
    Assert.Equal(Some(Time.fromUnixMillis 1758326400124L), Dbm.timestamp "1758326400123.6")
    Assert.Equal(None, Dbm.timestamp "")

[<Fact>]
let ``planSteps tells no definition, a list of steps, an EXPLAIN document and neither apart`` () =
    Assert.Equal(Dbm.NoDefinition, Dbm.planSteps None)
    // What the postgres check sends when it could not collect a plan.
    Assert.Equal(Dbm.NoDefinition, Dbm.planSteps (Some(json "null")))
    Assert.Equal(Dbm.Steps 0u, Dbm.planSteps (Some(json "[]")))
    Assert.Equal(Dbm.Steps 3u, Dbm.planSteps (Some(json """[{"id":1},{"id":2},{"id":3}]""")))
    Assert.Equal(Dbm.Document, Dbm.planSteps (Some(json "\"{\\\"Plan\\\":{}}\"")))
    Assert.Equal(Dbm.NotAPlan, Dbm.planSteps (Some(json "5")))

[<Fact>]
let ``a postgres plan, EXPLAIN output inside a string, is a plan and not an undecoded key`` () =
    let r =
        row
            "databasequery"
            """{"timestamp":1758326400000,"host":"pg1","ddsource":"postgres","dbm_type":"plan",
                "db":{"instance":"shop","statement":"SELECT ?","plan":{"signature":"sig-1","definition":"{\"Plan\":{\"Node Type\":\"Limit\"}}"}}}"""

    Assert.Equal(Some "sig-1", r.PlanSignature)
    Assert.Equal(None, r.PlanDefinitionSteps)
    Assert.Empty r.UndecodedKeys

[<Fact>]
let ``a plan whose definition is neither a list nor a document keeps its signature and is noted as undecoded`` () =
    let r =
        row
            "databasequery"
            """{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl","dbm_type":"plan",
                "db":{"instance":"orcl","plan":{"signature":"plan-xyz","definition":5}}}"""

    Assert.Equal(None, r.PlanDefinitionSteps)
    Assert.Equal(Some "plan-xyz", r.PlanSignature)
    Assert.Contains("db.plan.definition", r.UndecodedKeys)

[<Fact>]
let ``a db object with a field of the wrong type gives none of the sample columns`` () =
    let r = row "databasequery" """{"db":{"instance":"orcl","query_signature":7,"statement":"select 1"}}"""
    Assert.Equal(None, r.DBInstance)
    Assert.Equal(None, r.QuerySignature)
    Assert.Equal(None, r.Statement)
    Assert.Equal<string[]>([| "db" |], r.ExtraKeys)

[<Fact>]
let ``dbmmetrics: the oracle check's metrics payload`` () =
    let event =
        """{"host":"db1","database_instance":"db1/orcl","timestamp":1758326400000,
            "min_collection_interval":60,"tags":["env:prod","team:db"],
            "ddagentversion":"7.60.0","ddagenthostname":"agent-host-1",
            "oracle_rows":[{"query_signature":"sig1","sql_text":"select 1"}],"oracle_version":"19c"}"""

    let r = row "dbmmetrics" event
    Assert.Equal("test-tenant", r.TenantID)
    Assert.Equal("dbmmetrics", r.Track)
    Assert.Equal(receivedAt, r.ReceivedAt)
    Assert.Equal(Some(Time.fromUnixMillis 1758326400000L), r.Timestamp)
    Assert.Equal(event, r.Event)
    Assert.Equal("db1", r.Host)
    Assert.Equal("db1/orcl", r.DatabaseInstance)
    Assert.Equal("agent-host-1", r.AgentHostname)
    Assert.Equal("7.60.0", r.AgentVersion)
    Assert.Equal(Some 60.0, r.CollectionInterval)
    Assert.Equal<Map<string, string[]>>(Map [ "env", [| "prod" |]; "team", [| "db" |] ], r.Tags)
    Assert.Equal<string[]>([| "oracle_rows"; "oracle_version" |], r.ExtraKeys)

[<Fact>]
let ``dbmactivity: the oracle check's activity snapshot`` () =
    let r =
        row
            "dbmactivity"
            """{"host":"db1","database_instance":"db1/orcl","timestamp":1758326400000,
                "ddsource":"oracle","dbm_type":"activity","ddagentversion":"7.60.0",
                "ddtags":["env:prod"],"collection_interval":10,
                "oracle_activity":[{"session_id":1},{"session_id":2}]}"""

    Assert.Equal("oracle", r.Source)
    Assert.Equal("activity", r.DBMType)
    Assert.Equal(Some 10.0, r.CollectionInterval)
    Assert.Contains("oracle_activity", r.ExtraKeys)

[<Fact>]
let ``databasequery: a full query text sample fills the sample columns, and has no plan`` () =
    let r =
        row
            "databasequery"
            """{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl",
                "ddagentversion":"7.60.0","ddsource":"oracle","ddtags":"env:prod,team:db","dbm_type":"fqt",
                "db":{"instance":"orcl","query_signature":"abc123","statement":"select 1 from dual",
                      "metadata":{"dd_tables":["t1"],"dd_commands":["SELECT"]}}}"""

    Assert.Equal(Some "orcl", r.DBInstance)
    Assert.Equal(Some "abc123", r.QuerySignature)
    Assert.Equal(Some "select 1 from dual", r.Statement)
    Assert.Equal(None, r.PlanSignature)
    Assert.Equal(None, r.PlanDefinitionSteps)
    // The sample's metadata has no column; it is still there in the event.
    Assert.Contains("\"dd_tables\":[\"t1\"]", r.Event)

[<Fact>]
let ``databasequery: a plan's steps are counted`` () =
    let r =
        row
            "databasequery"
            """{"timestamp":1758326400000,"host":"db1","dbm_type":"plan",
                "db":{"instance":"orcl","query_signature":"abc123","statement":"select 1 from dual",
                      "plan":{"signature":"plan-xyz",
                              "definition":[{"id":1,"operation":"SELECT STATEMENT"},
                                            {"id":2,"operation":"TABLE ACCESS"},
                                            {"id":3,"operation":"INDEX SCAN"}]},
                      "metadata":{"tables":["t1"],"commands":["SELECT"]}}}"""

    Assert.Equal(Some "plan-xyz", r.PlanSignature)
    Assert.Equal(Some 3u, r.PlanDefinitionSteps)

[<Fact>]
let ``dbmmetadata: a database instance, with the agent_version spelling`` () =
    let r =
        row
            "dbmmetadata"
            """{"host":"db1","database_instance":"db1/orcl","agent_version":"7.60.0",
                "dbms":"oracle","kind":"database_instance","collection_interval":300,
                "dbms_version":"19c","tags":["env:prod"],"timestamp":1758326400000,
                "metadata":{"dbm":true,"connection_host":"db1"}}"""

    Assert.Equal("7.60.0", r.AgentVersion)
    Assert.Equal("oracle", r.DBMS)
    Assert.Equal("19c", r.DBMSVersion)
    Assert.Equal("database_instance", r.Kind)
    Assert.Contains("metadata", r.ExtraKeys)

[<Fact>]
let ``dbmhealth and dbmcolumnstatistics: keys the envelope does not know are kept by name`` () =
    let health =
        row
            "dbmhealth"
            """{"host":"db1","database_instance":"db1/orcl","timestamp":1758326400000,"ddagentversion":"7.60.0","ddtags":["env:prod"],"status":"ok"}"""

    Assert.Equal("dbmhealth", health.Track)
    Assert.Contains("status", health.ExtraKeys)

    let statistics =
        row
            "dbmcolumnstatistics"
            """{"host":"pg1","database_instance":"pg1/mydb","timestamp":1758326400000,"ddagentversion":"7.60.0",
                "ddtags":["env:staging"],
                "postgres_rows":[{"schema_name":"public","table_name":"users","column_name":"id"}]}"""

    Assert.Equal("dbmcolumnstatistics", statistics.Track)
    Assert.Contains("postgres_rows", statistics.ExtraKeys)
    Assert.Contains("schema_name", statistics.Event)

[<Fact>]
let ``an event without a timestamp gives a row without one`` () =
    Assert.Equal(None, (row "dbmhealth" """{"host":"db1"}""").Timestamp)

[<Fact>]
let ``a bare object is stored with its text exactly as sent`` () =
    let body = " {\"host\": \"db1\",\n  \"status\": \"ok\"}\n"
    let response, sink = post "/api/v2/dbmhealth" body
    Assert.Equal(202, response.Status)
    let stored = Assert.Single(sink.Rows<DBMEventRow>())
    Assert.Equal(body, stored.Event)
    Assert.Equal(Replay.testTenant, stored.TenantID)

[<Fact>]
let ``an element of an array is stored with its own text`` () =
    let _, sink = post "/api/v2/dbmhealth" """[ {"host" : "db1"} , {"host":"db2"} ]"""
    let events = sink.Rows<DBMEventRow>() |> List.map _.Event
    Assert.Equal<string list>([ """{"host" : "db1"}"""; """{"host":"db2"}""" ], events)

[<Fact>]
let ``a string Python could not encode is stored with U+FFFD, not answered with a 500`` () =
    // Python writes half a surrogate pair for a byte it could not decode.
    let response, sink = post "/api/v2/dbmhealth" """[{"host":"a\ud800b","ddtags":"k:\udc80,ok:1","k\ud800":1}]"""
    Assert.Equal(202, response.Status)
    let stored = Assert.Single(sink.Rows<DBMEventRow>())
    Assert.Equal("a\uFFFDb", stored.Host)
    Assert.Equal<Map<string, string[]>>(Map [ "k", [| "\uFFFD" |]; "ok", [| "1" |] ], stored.Tags)
    Assert.Equal<string[]>([| "k\uFFFD" |], stored.ExtraKeys)

[<Fact>]
let ``without a tenant nothing is stored and the answer is still 202`` () =
    let sink = CapturingSink()

    let request: Request =
        { Http = DefaultHttpContext()
          Body = Encoding.UTF8.GetBytes """{"host":"db1","database_instance":"db1/orcl"}"""
          Params = Map.empty
          Key = None
          Sink = sink
          Log = NullLogger.Instance }

    let response = Dbm.handle "dbmmetrics" request
    Assert.Equal(202, response.Status)
    Assert.Empty sink.Writes
