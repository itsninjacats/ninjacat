/// Tests of Routers/Evp.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.EvpTests

open System
open System.Text
open System.Text.Json
open Google.Protobuf
open Google.Protobuf.WellKnownTypes
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open Datadog.Agentdiscovery
open Datadog.Healthplatform
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Intake.Tests.Golden

let private now = DateTime(2026, 9, 23, 10, 30, 0, DateTimeKind.Utc)

let private utf8 (text: string) : byte[] = Encoding.UTF8.GetBytes text

let private parse (json: string) : JsonElement =
    match Evp.parseObject (utf8 json) with
    | Some value -> value
    | None -> failwith "the test's JSON is not an object"

let private withKey = [ "Dd-Api-Key", Replay.testKey ]

/// Sends one POST through the engine of a Go route set, as the Go handler
/// tests did, and returns the answer with what was written.
let private post (routeSet: string) (url: string) (headers: (string * string) list) (body: byte[]) : Response * CapturingSink =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let http = DefaultHttpContext()

    let path, query =
        match url.IndexOf '?' with
        | -1 -> url, ""
        | i -> url.Substring(0, i), url.Substring i

    http.Request.Method <- "POST"
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString path
    http.Request.QueryString <- QueryString query

    for name, value in headers do
        http.Request.Headers[name] <- StringValues value

    Routes.byGoNames deps [ routeSet ] http body, sink

let private assertAccepted (response: Response) =
    Assert.Equal(202, response.Status)
    Assert.Equal("{}", Encoding.UTF8.GetString response.Body)

let private rawPayloads (sink: CapturingSink) : RawPayloadRow list = sink.Rows<RawPayloadRow>()

// The "absent, not zero" rule every string + parsed-time pair here leans on.
[<Theory>]
[<InlineData("", false)>]
[<InlineData("not-a-timestamp", false)>]
[<InlineData("2026-09-23T10:30:00Z", true)>]
[<InlineData("2026-09-23T10:30:00.123456789Z", true)>]
[<InlineData("1758622200", false)>]
let ``a wire timestamp is parsed only when it is RFC 3339, never guessed`` (text: string, parses: bool) =
    Assert.Equal(parses, (Time.tryRfc3339 text).IsSome)

[<Fact>]
let ``every route answers an empty body with 202 and stores nothing`` () =
    let routes =
        [ "routeAgentDiscovery", "/api/v2/agentdiscovery"
          "routeAgentHealth", "/api/v2/agenthealth"
          "routeEventManagement", "/api/v2/events"
          "routeSoftwareInventory", "/api/v2/softinv"
          "routeSynthetics", "/api/v2/synthetics"
          "routeDataObs", "/api/v1/lineage"
          "routeDataObs", "/api/v2/query-actions" ]

    for routeSet, path in routes do
        let response, sink = post routeSet path withKey [||]
        assertAccepted response
        Assert.Empty sink.Writes

let private discoveryBatch () : AgentDiscoveryPayloadBatch =
    let postgres = AgentDiscoveryPayload(Integration = "postgres", Runtime = "python", RuntimeId = "rt-1")
    postgres.IngestionTimestamp <- Timestamp.FromDateTime(now.AddMinutes -1.0)

    postgres.ConfigFiles.Add(
        AgentDiscoveryConfigFile(
            Path = "/etc/datadog-agent/conf.d/postgres.yaml",
            Content = ByteString.CopyFromUtf8 "instances:\n  - host: db\n    password: hunter2\n",
            Truncated = false,
            PayloadFormat = AgentDiscoveryConfigFilePayloadFormat.PayloadFormatYaml
        )
    )

    postgres.ConfigFiles.Add(
        AgentDiscoveryConfigFile(
            Path = "/etc/x.conf",
            Content = ByteString.CopyFromUtf8 "...",
            Truncated = true,
            PayloadFormat = AgentDiscoveryConfigFilePayloadFormat.PayloadFormatUnknown
        )
    )

    postgres.EnvVars.Add(AgentDiscoveryEnvVar(Name = "PGPASSWORD", Value = "hunter2"))
    postgres.EnvVars.Add(AgentDiscoveryEnvVar(Name = "PGHOST", Value = "db"))

    let batch = AgentDiscoveryPayloadBatch(HostId = "host-1")
    batch.Payloads.Add postgres
    batch.Payloads.Add(AgentDiscoveryPayload(Integration = "redis", Runtime = "go", RuntimeId = "rt-2"))
    batch

[<Fact>]
let ``a discovery batch gives one row per payload, each with the batch's host id`` () =
    let rows = Evp.agentDiscoveryRows "acme" now (discoveryBatch ())
    Assert.Equal(2, rows.Length)

    let postgres = rows[0]
    Assert.Equal("acme", postgres.TenantID)
    Assert.Equal(now, postgres.ReceivedAt)
    Assert.Equal("host-1", postgres.HostID)
    Assert.Equal("postgres", postgres.Integration)
    Assert.Equal("python", postgres.Runtime)
    Assert.Equal("rt-1", postgres.RuntimeID)
    Assert.Equal(Some(now.AddMinutes -1.0), postgres.IngestionTimestamp)
    Assert.Equal("host-1", rows[1].HostID)

[<Fact>]
let ``config files travel as index-aligned arrays with their content kept as sent`` () =
    let postgres = (Evp.agentDiscoveryRows "acme" now (discoveryBatch ()))[0]
    Assert.Equal<string[]>([| "/etc/datadog-agent/conf.d/postgres.yaml"; "/etc/x.conf" |], postgres.ConfigPaths)

    Assert.Equal<string[]>(
        [| "instances:\n  - host: db\n    password: hunter2\n"; "..." |],
        postgres.ConfigContents |> Array.map Encoding.UTF8.GetString
    )

    Assert.Equal<uint8[]>([| 0uy; 1uy |], postgres.ConfigTruncated)
    Assert.Equal<string[]>([| "PAYLOAD_FORMAT_YAML"; "PAYLOAD_FORMAT_UNKNOWN" |], postgres.ConfigFormats)

[<Fact>]
let ``a config file that is not UTF-8 keeps its bytes`` () =
    let latin1 = [| 0x63uy; 0x61uy; 0x66uy; 0xE9uy |]
    let payload = AgentDiscoveryPayload(Integration = "x")
    payload.ConfigFiles.Add(AgentDiscoveryConfigFile(Path = "/etc/x", Content = ByteString.CopyFrom latin1))
    let batch = AgentDiscoveryPayloadBatch()
    batch.Payloads.Add payload

    let row = (Evp.agentDiscoveryRows "acme" now batch)[0]
    Assert.Equal<uint8[]>(latin1, row.ConfigContents[0])

[<Fact>]
let ``a config format the schema does not know is stored as its number`` () =
    let payload = AgentDiscoveryPayload()
    payload.ConfigFiles.Add(AgentDiscoveryConfigFile(PayloadFormat = enum<AgentDiscoveryConfigFilePayloadFormat> 42))
    payload.ConfigFiles.Add(AgentDiscoveryConfigFile(PayloadFormat = AgentDiscoveryConfigFilePayloadFormat.PayloadFormatRedisConf))
    let batch = AgentDiscoveryPayloadBatch()
    batch.Payloads.Add payload

    let row = (Evp.agentDiscoveryRows "acme" now batch)[0]
    Assert.Equal<string[]>([| "42"; "PAYLOAD_FORMAT_REDIS_CONF" |], row.ConfigFormats)

[<Fact>]
let ``env var values are kept, not only their names, in wire order`` () =
    let postgres = (Evp.agentDiscoveryRows "acme" now (discoveryBatch ()))[0]
    Assert.Equal<string[]>([| "PGPASSWORD"; "PGHOST" |], postgres.EnvVarNames)
    Assert.Equal<string[]>([| "hunter2"; "db" |], postgres.EnvVarValues)

[<Fact>]
let ``a payload without timestamp, files or env vars stores null and empty arrays`` () =
    let redis = (Evp.agentDiscoveryRows "acme" now (discoveryBatch ()))[1]
    Assert.Equal(None, redis.IngestionTimestamp)
    Assert.Empty redis.ConfigPaths
    Assert.Empty redis.ConfigContents
    Assert.Empty redis.ConfigTruncated
    Assert.Empty redis.ConfigFormats
    Assert.Empty redis.EnvVarNames
    Assert.Empty redis.EnvVarValues

// Env vars are a repeated field, not a map: a map would collapse the
// duplicate to its last value and lose the order.
[<Fact>]
let ``an env var name sent twice keeps both values`` () =
    let payload = AgentDiscoveryPayload(Integration = "postgres", Runtime = "python")
    payload.EnvVars.Add(AgentDiscoveryEnvVar(Name = "PATH", Value = "/usr/bin"))
    payload.EnvVars.Add(AgentDiscoveryEnvVar(Name = "PATH", Value = "/opt/datadog/bin"))
    let batch = AgentDiscoveryPayloadBatch(HostId = "host-1")
    batch.Payloads.Add payload

    let rows = Evp.agentDiscoveryRows "acme" now batch
    Assert.Equal(1, rows.Length)
    Assert.Equal<string[]>([| "PATH"; "PATH" |], rows[0].EnvVarNames)
    Assert.Equal<string[]>([| "/usr/bin"; "/opt/datadog/bin" |], rows[0].EnvVarValues)

[<Fact>]
let ``a discovery batch sent through the route is stored with the key's tenant`` () =
    let response, sink = post "routeAgentDiscovery" "/api/v2/agentdiscovery" withKey (discoveryBatch().ToByteArray())
    assertAccepted response

    let write = Assert.Single sink.Writes
    Assert.Equal("storage_agent_discovery", write.Writer)
    let rows = sink.Rows<AgentDiscoveryRow>()
    Assert.Equal<string list>([ "postgres"; "redis" ], rows |> List.map _.Integration)
    Assert.All(rows, (fun row -> Assert.Equal(Replay.testTenant, row.TenantID)))

[<Fact>]
let ``a discovery body that is not protobuf is kept raw as a decode error`` () =
    let body = [| 0xFFuy; 0xFFuy; 0xFFuy |]
    let response, sink = post "routeAgentDiscovery" "/api/v2/agentdiscovery" withKey body
    assertAccepted response

    let raw = Assert.Single(rawPayloads sink)
    Assert.Equal("agentdiscovery", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal<uint8[]>(body, raw.Body)
    Assert.Empty(sink.Rows<AgentDiscoveryRow>())

// Protobuf skips unknown fields, so a payload meant for another endpoint
// decodes into an empty batch: that is logged, not stored.
[<Fact>]
let ``a discovery batch that decodes to zero payloads stores nothing, not even raw`` () =
    let body = AgentDiscoveryPayloadBatch(HostId = "host-1").ToByteArray()
    let response, sink = post "routeAgentDiscovery" "/api/v2/agentdiscovery" withKey body
    assertAccepted response
    Assert.Empty sink.Writes

let private healthReport () : HealthReport =
    let extra = Struct()
    extra.Fields["probe"] <- Value.ForString "tcp"
    extra.Fields["attempts"] <- Value.ForNumber 3.0

    let resolved = Issue(Id = "z-issue", IssueName = "disk_full", Title = "Disk full", Severity = IssueSeverity.High, Extra = extra)
    resolved.Tags.Add "env:prod"
    resolved.Tags.Add "env:staging"
    resolved.PersistedIssue <- PersistedIssue(State = IssueState.Resolved, ResolvedAt = "2026-09-23T09:00:00Z")

    // No PersistedIssue at all.
    let unresolved = Issue(Id = "a-issue", IssueName = "cert_expiring", Severity = IssueSeverity.Medium)

    let report =
        HealthReport(SchemaVersion = "1.0", EventType = "health_report", EmittedAt = "2026-09-23T10:29:00Z", Service = "logs-agent")

    report.Host <- HostInfo(Hostname = "web-1")
    report.Host.ParIds.Add "par-1"
    report.Issues["z-issue"] <- resolved
    report.Issues["a-issue"] <- unresolved
    report

let private reportID = Guid.Parse "11111111-1111-1111-1111-111111111111"

[<Fact>]
let ``a health report row carries its host, its issue count and the parsed emit time`` () =
    let row, _ = Evp.healthReportRows "acme" now reportID (healthReport ()) None
    Assert.Equal("acme", row.TenantID)
    Assert.Equal(now, row.ReceivedAt)
    Assert.Equal(reportID, row.ReportID)
    Assert.Equal("1.0", row.SchemaVersion)
    Assert.Equal("health_report", row.EventType)
    Assert.Equal("logs-agent", row.Service)
    Assert.Equal("web-1", row.Host)
    Assert.Equal<string[]>([| "par-1" |], row.ParIDs)
    Assert.Equal(None, row.AgentVersion)
    Assert.Equal(2u, row.IssueCount)
    Assert.Equal("2026-09-23T10:29:00Z", row.EmittedAt)
    Assert.Equal(Some(DateTime(2026, 9, 23, 10, 29, 0, DateTimeKind.Utc)), row.EmittedAtParsed)

[<Fact>]
let ``a health report without a host stores empty host fields`` () =
    let row, issues = Evp.healthReportRows "acme" now reportID (HealthReport()) None
    Assert.Equal("", row.Host)
    Assert.Equal(None, row.AgentVersion)
    Assert.Empty row.ParIDs
    Assert.Equal(0u, row.IssueCount)
    Assert.Equal(None, row.EmittedAtParsed)
    Assert.Empty issues

[<Fact>]
let ``health issues come out sorted by key, each with the report's id`` () =
    let _, issues = Evp.healthReportRows "acme" now reportID (healthReport ()) None
    Assert.Equal<string[]>([| "a-issue"; "z-issue" |], issues |> Array.map _.IssueKey)
    Assert.All(issues, (fun issue -> Assert.Equal(reportID, issue.ReportID)))

[<Fact>]
let ``an issue without a lifecycle stores no state and no resolved time`` () =
    let _, issues = Evp.healthReportRows "acme" now reportID (healthReport ()) None
    let noLifecycle = issues[0]
    Assert.Equal(None, noLifecycle.ResolvedAt)
    Assert.Equal("", noLifecycle.PersistedState)
    Assert.Equal("ISSUE_SEVERITY_MEDIUM", noLifecycle.Severity)
    Assert.Equal("", noLifecycle.Extra)
    Assert.Equal(None, noLifecycle.ScriptRequiresRoot)
    Assert.Empty noLifecycle.RemediationStepOrder

[<Fact>]
let ``an issue with a lifecycle stores enum names, multi-valued tags and its extra as JSON`` () =
    let _, issues = Evp.healthReportRows "acme" now reportID (healthReport ()) None
    let withLifecycle = issues[1]
    Assert.Equal(Some "2026-09-23T09:00:00Z", withLifecycle.ResolvedAt)
    Assert.Equal("ISSUE_STATE_RESOLVED", withLifecycle.PersistedState)
    Assert.Equal("ISSUE_SEVERITY_HIGH", withLifecycle.Severity)
    Assert.Equal<string[]>([| "prod"; "staging" |], withLifecycle.Tags["env"])
    Assert.Equal("""{"attempts":3,"probe":"tcp"}""", withLifecycle.Extra)

[<Fact>]
let ``a remediation's steps and script are stored, and a script that needs no root says so`` () =
    let remediation = Remediation(Summary = "free some space")
    remediation.Steps.Add(RemediationStep(Order = 1, Text = "find big files"))
    remediation.Steps.Add(RemediationStep(Order = 2, Text = "delete them"))
    remediation.Script <- Script(Language = "bash", LanguageVersion = "5+", Filename = "clean.sh", Content = "rm -rf /tmp/x")
    let report = HealthReport()
    report.Issues["disk"] <- Issue(Remediation = remediation)

    let _, issues = Evp.healthReportRows "acme" now reportID report None
    let row = issues[0]
    Assert.Equal("free some space", row.RemediationSummary)
    Assert.Equal<int[]>([| 1; 2 |], row.RemediationStepOrder)
    Assert.Equal<string[]>([| "find big files"; "delete them" |], row.RemediationStepText)
    Assert.Equal("bash", row.ScriptLanguage)
    Assert.Equal("5+", row.ScriptLanguageVersion)
    Assert.Equal("clean.sh", row.ScriptFilename)
    Assert.Equal("rm -rf /tmp/x", row.ScriptContent)
    Assert.Equal(Some 0uy, row.ScriptRequiresRoot)

let private decoded (json: string) : HealthReport =
    match Evp.decodeHealthReport (utf8 json) with
    | Ok report -> report
    | Error e -> failwith $"the report did not decode: {e}"

// Extra is a protobuf Struct, which holds every number as a float64. An id
// above 2^53 survives only because Extra is taken from the raw JSON.
[<Fact>]
let ``an issue's extra keeps a number above 2^53 digit for digit`` () =
    let body =
        """{
            "issues": {
                "conn-refused": {
                    "id": "conn-refused",
                    "extra": {"probe": "tcp", "probe_pid": 9007199254740993}
                }
            }
        }"""

    let _, issues = Evp.healthReportRows "acme" now reportID (decoded body) (Evp.healthIssuesRaw (utf8 body))
    let issue = Assert.Single issues
    Assert.Equal("conn-refused", issue.ID)
    Assert.Equal("""{"probe":"tcp","probe_pid":9007199254740993}""", issue.Extra)

[<Fact>]
let ``without the raw JSON an issue's extra falls back to what protobuf decoded`` () =
    let body = """{"issues": {"a": {"extra": {"probe": "tcp", "attempts": 3}}}}"""
    let _, issues = Evp.healthReportRows "acme" now reportID (decoded body) None
    Assert.Equal("""{"attempts":3,"probe":"tcp"}""", issues[0].Extra)

[<Fact>]
let ``a health report decodes snake_case keys and enums sent as numbers`` () =
    let report =
        decoded
            """{
                "schema_version": "1.0", "event_type": "health_report", "emitted_at": "2026-09-23T10:29:00Z",
                "service": "datadog-agent", "unknown_key": {"ignored": true},
                "host": {"hostname": "web-1", "agent_version": "7.83.0", "par_ids": ["p1", "p2"]},
                "issues": {
                    "disk": {
                        "id": "disk", "issue_name": "disk_full", "title": "Disk full", "description": "d",
                        "category": "storage", "location": "core-agent", "severity": 3,
                        "detected_at": "2026-09-23T10:00:00.5Z", "source": "logs", "tags": ["env:prod", "team:a"],
                        "issue_type": "disk_full",
                        "remediation": {
                            "summary": "s", "steps": [{"order": 2, "text": "t"}],
                            "script": {"language": "bash", "language_version": "5", "filename": "f.sh",
                                       "requires_root": true, "content": "c"}
                        },
                        "persisted_issue": {"state": 4, "first_seen": "f", "last_seen": "l", "resolved_at": "r"}
                    }
                }
            }"""

    let reportRow, issues = Evp.healthReportRows "acme" now reportID report None
    Assert.Equal("1.0", reportRow.SchemaVersion)
    Assert.Equal("health_report", reportRow.EventType)
    Assert.Equal("datadog-agent", reportRow.Service)
    Assert.Equal("web-1", reportRow.Host)
    Assert.Equal(Some "7.83.0", reportRow.AgentVersion)
    Assert.Equal<string[]>([| "p1"; "p2" |], reportRow.ParIDs)

    let issue = Assert.Single issues
    Assert.Equal("disk", issue.IssueKey)
    Assert.Equal("disk", issue.ID)
    Assert.Equal("disk_full", issue.IssueName)
    Assert.Equal("Disk full", issue.Title)
    Assert.Equal("d", issue.Description)
    Assert.Equal("storage", issue.Category)
    Assert.Equal("core-agent", issue.Location)
    Assert.Equal("ISSUE_SEVERITY_HIGH", issue.Severity)
    Assert.Equal("2026-09-23T10:00:00.5Z", issue.DetectedAt)
    Assert.Equal(Some(DateTime(2026, 9, 23, 10, 0, 0, 500, DateTimeKind.Utc)), issue.DetectedAtParsed)
    Assert.Equal("logs", issue.Source)
    Assert.Equal<string[]>([| "prod" |], issue.Tags["env"])
    Assert.Equal<string[]>([| "a" |], issue.Tags["team"])
    Assert.Equal("disk_full", issue.IssueType)
    Assert.Equal("s", issue.RemediationSummary)
    Assert.Equal<int[]>([| 2 |], issue.RemediationStepOrder)
    Assert.Equal<string[]>([| "t" |], issue.RemediationStepText)
    Assert.Equal("bash", issue.ScriptLanguage)
    Assert.Equal("5", issue.ScriptLanguageVersion)
    Assert.Equal("f.sh", issue.ScriptFilename)
    Assert.Equal(Some 1uy, issue.ScriptRequiresRoot)
    Assert.Equal("c", issue.ScriptContent)
    Assert.Equal("ISSUE_STATE_ACTIVE", issue.PersistedState)
    Assert.Equal("f", issue.FirstSeen)
    Assert.Equal("l", issue.LastSeen)
    Assert.Equal(Some "r", issue.ResolvedAt)

// What Go's encoding/json does with the same bodies, checked against Go.
[<Fact>]
let ``a health report's keys match ignoring case, and null leaves a field empty`` () =
    let report =
        decoded
            """{"Schema_Version": "1.0", "HOST": {"hostname": "h", "agent_version": ""},
                "issues": {"a": null,
                           "b": {"severity": 7, "extra": null, "tags": ["x", null],
                                 "remediation": {"steps": [null, {"order": 1, "text": "t"}], "script": null},
                                 "persisted_issue": {"state": 4, "resolved_at": null}}}}"""

    let body = utf8 """{"issues": {"a": null, "b": {"extra": null}}}"""
    let reportRow, issues = Evp.healthReportRows "acme" now reportID report (Evp.healthIssuesRaw body)
    Assert.Equal("1.0", reportRow.SchemaVersion)
    Assert.Equal("h", reportRow.Host)
    // Sent, though empty: not the same as never sent.
    Assert.Equal(Some "", reportRow.AgentVersion)
    Assert.Equal(2u, reportRow.IssueCount)

    let nullIssue = issues[0]
    Assert.Equal("a", nullIssue.IssueKey)
    Assert.Equal("", nullIssue.ID)
    Assert.Equal("ISSUE_SEVERITY_UNSPECIFIED", nullIssue.Severity)
    Assert.Equal("", nullIssue.PersistedState)
    Assert.Equal("", nullIssue.Extra)

    let issue = issues[1]
    Assert.Equal("7", issue.Severity)
    Assert.Equal("", issue.Extra)
    // A null tag reads as an empty one.
    Assert.Equal<string list>([ ""; "x" ], issue.Tags.Keys |> List.ofSeq)
    Assert.Equal<int[]>([| 1 |], issue.RemediationStepOrder)
    Assert.Equal(None, issue.ScriptRequiresRoot)
    Assert.Equal("ISSUE_STATE_ACTIVE", issue.PersistedState)
    Assert.Equal(None, issue.ResolvedAt)

[<Fact>]
let ``a health report of null decodes to an empty report`` () =
    let reportRow, issues = Evp.healthReportRows "acme" now reportID (decoded "null") (Evp.healthIssuesRaw (utf8 "null"))
    Assert.Equal("", reportRow.SchemaVersion)
    Assert.Equal(0u, reportRow.IssueCount)
    Assert.Empty issues

[<Theory>]
[<InlineData("""[1]""")>]
[<InlineData(""""text" """)>]
[<InlineData("""{"schema_version": 5}""")>]
[<InlineData("""{"host": []}""")>]
[<InlineData("""{"host": {"par_ids": "p1"}}""")>]
[<InlineData("""{"issues": []}""")>]
[<InlineData("""{"issues": {"a": 1}}""")>]
[<InlineData("""{"issues": {"a": {"severity": "ISSUE_SEVERITY_HIGH"}}}""")>]
[<InlineData("""{"issues": {"a": {"severity": 1.0}}}""")>]
[<InlineData("""{"issues": {"a": {"severity": 4294967296}}}""")>]
[<InlineData("""{"issues": {"a": {"tags": "x"}}}""")>]
[<InlineData("""{"issues": {"a": {"tags": [1]}}}""")>]
[<InlineData("""{"issues": {"a": {"extra": 5}}}""")>]
[<InlineData("""{"issues": {"a": {"extra": [1]}}}""")>]
[<InlineData("""{"issues": {"a": {"remediation": {"steps": [1]}}}}""")>]
[<InlineData("""{"issues": {"a": {"remediation": {"script": {"requires_root": "yes"}}}}}""")>]
[<InlineData("""{"issues": {"a": {"persisted_issue": {"state": "ISSUE_STATE_ACTIVE"}}}}""")>]
[<InlineData("""{"schema_version": """)>]
let ``a health report with a value of the wrong type is refused whole`` (body: string) =
    Assert.True((Evp.decodeHealthReport (utf8 body)).IsError)

[<Fact>]
let ``a health report sent through the route stores the report, then its issues`` () =
    let body =
        """{"schema_version": "1.0", "host": {"hostname": "web-1"},
            "issues": {"b": {"id": "b", "severity": 1, "extra": {"pid": 9007199254740993}}, "a": {"id": "a"}}}"""

    let response, sink = post "routeAgentHealth" "/api/v2/agenthealth" withKey (utf8 body)
    assertAccepted response
    Assert.Equal<string list>([ "storage_agent_health_reports"; "storage_agent_health_issues" ], sink.Writes |> List.map _.Writer)

    let report = Assert.Single(sink.Rows<AgentHealthReportRow>())
    Assert.Equal(Replay.testTenant, report.TenantID)
    Assert.Equal("web-1", report.Host)
    Assert.Equal(2u, report.IssueCount)
    Assert.NotEqual(Guid.Empty, report.ReportID)

    let issues = sink.Rows<AgentHealthIssueRow>()
    Assert.Equal<string list>([ "a"; "b" ], issues |> List.map _.IssueKey)
    Assert.All(issues, (fun issue -> Assert.Equal(report.ReportID, issue.ReportID)))
    Assert.All(issues, (fun issue -> Assert.Equal(report.ReceivedAt, issue.ReceivedAt)))
    Assert.Equal("ISSUE_SEVERITY_LOW", issues[1].Severity)
    Assert.Equal("""{"pid":9007199254740993}""", issues[1].Extra)

[<Fact>]
let ``a health report without issues stores only the report`` () =
    let response, sink = post "routeAgentHealth" "/api/v2/agenthealth" withKey (utf8 """{"service": "datadog-agent"}""")
    assertAccepted response
    Assert.Equal<string list>([ "storage_agent_health_reports" ], sink.Writes |> List.map _.Writer)

[<Fact>]
let ``a health report that does not decode is kept raw as a decode error`` () =
    let body = utf8 """{"issues": {"a": {"severity": "ISSUE_SEVERITY_HIGH"}}}"""
    let response, sink = post "routeAgentHealth" "/api/v2/agenthealth" withKey body
    assertAccepted response

    let raw = Assert.Single(rawPayloads sink)
    Assert.Equal("agenthealth", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal<uint8[]>(body, raw.Body)
    Assert.NotEqual<string>("", raw.Note)
    Assert.Empty(sink.Rows<AgentHealthReportRow>())

let private eventEnvelope =
    """{
        "data": {
            "type": "event",
            "id": "evt-123",
            "attributes": {
                "host": "web-1",
                "title": "High CPU",
                "category": "performance",
                "integration_id": "system",
                "message": "cpu above 90%",
                "timestamp": "2026-09-23T10:30:00Z",
                "tags": ["env:prod", "env:staging"],
                "aggregation_key": "cpu-web-1",
                "system-notable-events": {"event_type": "anomaly"},
                "attributes": {"status": "warn", "priority": "high"},
                "unknown_field": {"nested": true}
            }
        },
        "meta": {"page": 1}
    }"""

let private eventRow (json: string) : EventManagementEventRow =
    match Evp.eventManagementRow "acme" now (parse json) with
    | Some row -> row
    | None -> failwith "the envelope gave no row"

[<Fact>]
let ``an event envelope's documented attributes become columns`` () =
    let row = eventRow eventEnvelope
    Assert.Equal("acme", row.TenantID)
    Assert.Equal(now, row.ReceivedAt)
    Assert.Equal("event", row.DataType)
    Assert.Equal("web-1", row.Host)
    Assert.Equal("High CPU", row.Title)
    Assert.Equal("performance", row.Category)
    Assert.Equal("system", row.IntegrationID)
    Assert.Equal("cpu above 90%", row.Message)
    Assert.Equal("2026-09-23T10:30:00Z", row.Timestamp)
    Assert.Equal(Some now, row.TimestampParsed)
    Assert.Equal("cpu-web-1", row.AggregationKey)
    Assert.Equal<string[]>([| "prod"; "staging" |], row.Tags["env"])

// The outer attributes and the producer's own inner "attributes" object must
// not be confused, and the hyphenated key must be read as it is spelled.
[<Fact>]
let ``an event's inner attributes stay nested and its notable event type is found`` () =
    let row = eventRow eventEnvelope
    Assert.Equal("anomaly", row.NotableEventType)
    Assert.Equal("""{"priority":"high","status":"warn"}""", row.Attributes)

[<Fact>]
let ``an event's undeclared keys of all three levels land in outer_extra, prefixed by level`` () =
    let row = eventRow eventEnvelope
    Assert.Equal("""{"nested":true}""", row.OuterExtra["unknown_field"])
    Assert.Equal("\"evt-123\"", row.OuterExtra["data.id"])
    Assert.Equal("""{"page":1}""", row.OuterExtra["top.meta"])
    Assert.Equal(3, row.OuterExtra.Count)
    Assert.False(row.OuterExtra.ContainsKey "host")

[<Theory>]
[<InlineData("""{"data": {"type": "event"}}""")>]
[<InlineData("""{"data": {"type": "event", "attributes": "text"}}""")>]
[<InlineData("""{"data": []}""")>]
[<InlineData("""{}""")>]
[<InlineData("""null""")>]
let ``an event envelope without a data.attributes object gives no row`` (json: string) =
    Assert.Equal(None, Evp.eventManagementRow "acme" now (parse json))

// Go marshals a nil map here, which is the text "null". Pinned so that the
// port keeps storing what the Go server stored.
[<Fact>]
let ``an event without inner attributes stores the text null for them`` () =
    let row = eventRow """{"data": {"attributes": {"title": "t"}}}"""
    Assert.Equal("null", row.Attributes)
    Assert.Equal("", row.DataType)
    Assert.Equal("", row.Timestamp)
    Assert.Equal(None, row.TimestampParsed)
    Assert.True row.Tags.IsEmpty
    Assert.True row.OuterExtra.IsEmpty

[<Fact>]
let ``event values that are not strings are stored as sent`` () =
    let row =
        eventRow
            """{"data": {"type": 7, "attributes": {
                "host": 9007199254740993, "title": true, "message": {"b": 1, "a": [1.50, null]},
                "timestamp": 1758622200, "tags": "env:prod", "aggregation_key": null,
                "system-notable-events": "not-an-object", "extra_null": null}}}"""

    Assert.Equal("7", row.DataType)
    Assert.Equal("9007199254740993", row.Host)
    Assert.Equal("true", row.Title)
    Assert.Equal("""{"a":[1.50,null],"b":1}""", row.Message)
    Assert.Equal("1758622200", row.Timestamp)
    Assert.Equal(None, row.TimestampParsed)
    // A single tag sent as a bare string is a list of one.
    Assert.Equal<string[]>([| "prod" |], row.Tags["env"])
    Assert.Equal("", row.AggregationKey)
    Assert.Equal("", row.NotableEventType)
    // An undeclared key whose value is null is kept, with nothing in it.
    Assert.Equal("", row.OuterExtra["extra_null"])

[<Fact>]
let ``an event sent through the route is stored with the key's tenant`` () =
    let response, sink = post "routeEventManagement" "/api/v2/events" withKey (utf8 eventEnvelope)
    assertAccepted response

    let write = Assert.Single sink.Writes
    Assert.Equal("storage_event_management_events", write.Writer)
    let row = Assert.Single(sink.Rows<EventManagementEventRow>())
    Assert.Equal(Replay.testTenant, row.TenantID)
    Assert.Equal("High CPU", row.Title)

[<Theory>]
[<InlineData("""[{"data": {}}]""")>]
[<InlineData(""""text" """)>]
[<InlineData("""{"data": """)>]
let ``an event body that is not a JSON object is kept raw as a decode error`` (body: string) =
    let response, sink = post "routeEventManagement" "/api/v2/events" withKey (utf8 body)
    assertAccepted response

    let raw = Assert.Single(rawPayloads sink)
    Assert.Equal("event-management", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal("body is not a JSON object", raw.Note)
    Assert.Equal<uint8[]>(utf8 body, raw.Body)
    Assert.Empty(sink.Rows<EventManagementEventRow>())

// Go's Decoder reads the first value and never looks at what follows it.
[<Fact>]
let ``an event body is read up to the end of its first JSON value`` () =
    let body = utf8 """{"data": {"attributes": {"title": "first"}}} {"data": 1}"""
    let response, sink = post "routeEventManagement" "/api/v2/events" withKey body
    assertAccepted response
    Assert.Equal("first", (Assert.Single(sink.Rows<EventManagementEventRow>())).Title)
    Assert.Empty(rawPayloads sink)

let private softwarePayload =
    """{
        "hostname": "web-1",
        "extra_payload_field": "seen",
        "host_software": {
            "collected_at": "2026-09-23T10:00:00Z",
            "software": [
                {"software_type": "package", "name": "curl", "version": "7.88.1",
                 "publisher": "curl", "deployment_status": "installed",
                 "deployment_time": "2026-09-20T00:00:00Z", "product_code": "curl",
                 "is_64_bit": true, "install_paths": ["/usr/bin/curl"]},
                {"software_type": "package", "name": "mystery",
                 "vendor_specific_field": "x"},
                "not-an-object"
            ]
        }
    }"""

[<Fact>]
let ``a software payload gives one row per entry and drops an entry that is not an object`` () =
    let rows = Evp.hostSoftwareRows "acme" now (parse softwarePayload)
    Assert.Equal<string list>([ "curl"; "mystery" ], rows |> List.map _.Name)
    Assert.All(rows, (fun row -> Assert.Equal("web-1", row.Hostname)))
    Assert.All(rows, (fun row -> Assert.Equal("acme", row.TenantID)))
    Assert.All(rows, (fun row -> Assert.Equal(now, row.ReceivedAt)))

[<Fact>]
let ``a software entry's documented fields become columns`` () =
    let curl = (Evp.hostSoftwareRows "acme" now (parse softwarePayload))[0]
    Assert.Equal("package", curl.SoftwareType)
    Assert.Equal("7.88.1", curl.Version)
    Assert.Equal("curl", curl.Publisher)
    Assert.Equal("installed", curl.DeploymentStatus)
    Assert.Equal("2026-09-20T00:00:00Z", curl.DeploymentTime)
    Assert.Equal(Some(DateTime(2026, 9, 20, 0, 0, 0, DateTimeKind.Utc)), curl.DeploymentTimeParsed)
    Assert.Equal("curl", curl.ProductCode)
    Assert.Equal(Some 1uy, curl.Is64Bit)
    Assert.Equal<string[]>([| "/usr/bin/curl" |], curl.InstallPaths)
    Assert.True curl.Extra.IsEmpty

// Unknown bitness is common and real: false would claim 32-bit.
[<Fact>]
let ``a software entry that never said its bitness stores null, and keeps its undeclared keys`` () =
    let mystery = (Evp.hostSoftwareRows "acme" now (parse softwarePayload))[1]
    Assert.Equal(None, mystery.Is64Bit)
    Assert.Equal(None, mystery.DeploymentTimeParsed)
    Assert.Empty mystery.InstallPaths
    Assert.Equal("\"x\"", mystery.Extra["vendor_specific_field"])
    Assert.Equal(1, mystery.Extra.Count)

[<Fact>]
let ``a software payload's undeclared keys repeat onto every row, host_software's own under a prefix`` () =
    let rows = Evp.hostSoftwareRows "acme" now (parse softwarePayload)

    for row in rows do
        Assert.Equal("\"seen\"", row.PayloadExtra["extra_payload_field"])
        Assert.Equal("\"2026-09-23T10:00:00Z\"", row.PayloadExtra["host_software.collected_at"])
        Assert.Equal(2, row.PayloadExtra.Count)

[<Fact>]
let ``software bitness is read only from a JSON bool`` () =
    let rows =
        Evp.hostSoftwareRows
            "acme"
            now
            (parse """{"host_software": {"software": [{"is_64_bit": false}, {"is_64_bit": "true"}, {"is_64_bit": 1}]}}""")

    Assert.Equal<uint8 option list>([ Some 0uy; None; None ], rows |> List.map _.Is64Bit)

[<Theory>]
[<InlineData("""{"hostname": "web-1"}""")>]
[<InlineData("""{"hostname": "web-1", "host_software": {"software": []}}""")>]
[<InlineData("""{"hostname": "web-1", "host_software": {"software": {"name": "curl"}}}""")>]
[<InlineData("""{"hostname": "web-1", "host_software": "text"}""")>]
[<InlineData("""null""")>]
let ``a software payload without a software list gives no rows`` (json: string) =
    Assert.Empty(Evp.hostSoftwareRows "acme" now (parse json))

let private syntheticsResult =
    """{
        "test": {"id": "t-1", "name": "check google", "type": "api", "subType": "http", "version": "1"},
        "location": {"id": "loc-1", "name": "aws:eu-west-1", "displayName": "EU West"},
        "result": {
            "id": "r-1", "initialId": "r-1", "status": "passed", "runType": "scheduled",
            "duration": 123.45,
            "testStartedAt": "2026-09-23T10:30:00Z",
            "testFinishedAt": "2026-09-23T10:30:01Z",
            "assertions": [
                {"type": "statusCode", "operator": "is", "expected": 200, "actual": 200, "valid": true},
                {"type": "responseTime", "operator": "lessThan", "expected": 1000, "actual": 1500, "valid": false}
            ],
            "netstats": {"packetsSent": 9007199254740993, "packetsReceived": 10, "packetLossPercentage": 0.5,
                "jitter": 1.2, "latency": 42.5, "hops": 3}
        },
        "_dd": {"origin": "synthetics"},
        "v": 5
    }"""

[<Fact>]
let ``a synthetics result's test, location and result fields become columns`` () =
    let row = Evp.syntheticsResultRow "acme" now (parse syntheticsResult)
    Assert.Equal("acme", row.TenantID)
    Assert.Equal(now, row.ReceivedAt)
    Assert.Equal("t-1", row.TestID)
    Assert.Equal("check google", row.TestName)
    Assert.Equal("api", row.TestType)
    Assert.Equal("http", row.TestSubtype)
    Assert.Equal("1", row.TestVersion)
    Assert.Equal("loc-1", row.LocationID)
    Assert.Equal("aws:eu-west-1", row.LocationName)
    Assert.Equal("EU West", row.LocationDisplayName)
    Assert.Equal("r-1", row.ResultID)
    Assert.Equal("r-1", row.ResultInitialID)
    Assert.Equal("passed", row.Status)
    Assert.Equal("scheduled", row.RunType)
    Assert.Equal("123.45", row.Duration)
    Assert.Equal("2026-09-23T10:30:00Z", row.TestStartedAt)
    Assert.Equal(Some now, row.TestStartedAtParsed)
    Assert.Equal(Some(now.AddSeconds 1.0), row.TestFinishedAtParsed)
    Assert.Equal("", row.TestTriggeredAt)
    Assert.Equal(None, row.TestTriggeredAtParsed)
    Assert.Equal("""{"origin":"synthetics"}""", row.DD)
    Assert.Equal("5", row.V)
    Assert.Equal("", row.Config)
    Assert.Equal("", row.Netpath)
    Assert.Equal("", row.Enrichment)
    // Every key here is a documented one.
    Assert.True row.Extra.IsEmpty

[<Fact>]
let ``synthetics assertions keep their order and count as five aligned arrays`` () =
    let row = Evp.syntheticsResultRow "acme" now (parse syntheticsResult)
    Assert.Equal<string[]>([| "statusCode"; "responseTime" |], row.AssertionType)
    Assert.Equal<string[]>([| "is"; "lessThan" |], row.AssertionOperator)
    Assert.Equal<string[]>([| "200"; "1000" |], row.AssertionExpected)
    Assert.Equal<string[]>([| "200"; "1500" |], row.AssertionActual)
    Assert.Equal<uint8[]>([| 1uy; 0uy |], row.AssertionValid)

[<Fact>]
let ``an assertion that is not an object still takes its place in the arrays`` () =
    let row =
        Evp.syntheticsResultRow
            "acme"
            now
            (parse """{"result": {"assertions": ["text", {"type": "t", "expected": {"b": 1, "a": "x"}, "valid": "true"}]}}""")

    Assert.Equal<string[]>([| ""; "t" |], row.AssertionType)
    Assert.Equal<string[]>([| ""; "" |], row.AssertionOperator)
    Assert.Equal<string[]>([| ""; """{"a":"x","b":1}""" |], row.AssertionExpected)
    Assert.Equal<string[]>([| ""; "" |], row.AssertionActual)
    // Only a JSON true is valid.
    Assert.Equal<uint8[]>([| 0uy; 0uy |], row.AssertionValid)

// A counter one above 2^53 survives only if it never passes through a float.
[<Fact>]
let ``synthetics netstats keep a 64-bit counter exactly`` () =
    let row = Evp.syntheticsResultRow "acme" now (parse syntheticsResult)
    Assert.Equal(Some 9007199254740993L, row.NetstatsPacketsSent)
    Assert.Equal(Some 10L, row.NetstatsPacketsReceived)
    Assert.Equal(Some 0.5, row.NetstatsPacketLossPercentage)
    Assert.Equal(Some 1.2, row.NetstatsJitter)
    Assert.Equal(Some 42.5, row.NetstatsLatency)
    Assert.Equal(Some 3L, row.NetstatsHops)

[<Fact>]
let ``a netstat that is not a number of the column's kind stores null`` () =
    let row =
        Evp.syntheticsResultRow
            "acme"
            now
            (parse
                """{"result": {"netstats": {"packetsSent": 3.0, "packetsReceived": "10", "hops": 1e3,
                    "packetLossPercentage": "0.5", "jitter": 1e999, "latency": 7}}}""")

    Assert.Equal(None, row.NetstatsPacketsSent)
    Assert.Equal(None, row.NetstatsPacketsReceived)
    Assert.Equal(None, row.NetstatsHops)
    Assert.Equal(None, row.NetstatsPacketLossPercentage)
    Assert.Equal(None, row.NetstatsJitter)
    Assert.Equal(Some 7.0, row.NetstatsLatency)

// The failure object being there at all is the signal.
[<Fact>]
let ``a synthetics result without a failure object stores null for it, not empty text`` () =
    let row = Evp.syntheticsResultRow "acme" now (parse syntheticsResult)
    Assert.Equal(None, row.FailureCode)
    Assert.Equal(None, row.FailureMessage)

[<Fact>]
let ``a synthetics result with a failure object stores its code and message`` () =
    let row = Evp.syntheticsResultRow "acme" now (parse """{"result":{"failure":{"code":"TIMEOUT","message":"no response"}}}""")
    Assert.Equal(Some "TIMEOUT", row.FailureCode)
    Assert.Equal(Some "no response", row.FailureMessage)

    let empty = Evp.syntheticsResultRow "acme" now (parse """{"result":{"failure":{}}}""")
    Assert.Equal(Some "", empty.FailureCode)
    Assert.Equal(Some "", empty.FailureMessage)

[<Fact>]
let ``an undeclared key directly on the result lands in extra under a prefix`` () =
    let row =
        Evp.syntheticsResultRow "acme" now (parse """{"result":{"status":"passed","region":"eu-west-1"},"region":"top"}""")

    Assert.Equal("\"eu-west-1\"", row.Extra["result.region"])
    Assert.Equal("\"top\"", row.Extra["region"])
    Assert.Equal(2, row.Extra.Count)

[<Fact>]
let ``synthetics config, netpath and enrichment are kept as JSON with numbers digit for digit`` () =
    let row =
        Evp.syntheticsResultRow
            "acme"
            now
            (parse
                """{"result": {"duration": "12ms", "config": {"request": {"port": 443, "host": "example.com", "timeout": 1.50}},
                                "netpath": {"hops": [{"ttl": 1}], "id": 9007199254740993}},
                    "enrichment": [1, "two"]}""")

    Assert.Equal("12ms", row.Duration)
    Assert.Equal("""{"request":{"host":"example.com","port":443,"timeout":1.50}}""", row.Config)
    Assert.Equal("""{"hops":[{"ttl":1}],"id":9007199254740993}""", row.Netpath)
    Assert.Equal("""[1,"two"]""", row.Enrichment)

[<Fact>]
let ``synthetics results sent through the route are stored, and a bad item is kept raw`` () =
    let body = utf8 ("[" + syntheticsResult + """, 42, {"test": {"id": "t-2"}}]""")
    let response, sink = post "routeSynthetics" "/api/v2/synthetics" withKey body
    assertAccepted response
    Assert.Equal<string list>([ "storage_raw_payloads"; "storage_synthetics_results" ], sink.Writes |> List.map _.Writer)

    let rows = sink.Rows<SyntheticsResultRow>()
    Assert.Equal<string list>([ "t-1"; "t-2" ], rows |> List.map _.TestID)
    Assert.All(rows, (fun row -> Assert.Equal(Replay.testTenant, row.TenantID)))
    Assert.Equal(rows[0].ReceivedAt, rows[1].ReceivedAt)

    let raw = Assert.Single(rawPayloads sink)
    Assert.Equal("synthetics", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal("item is not a JSON object", raw.Note)
    Assert.Equal("42", Encoding.UTF8.GetString raw.Body)

let private lineageEvent =
    """{
        "eventType": "COMPLETE",
        "eventTime": "2026-09-23T10:30:00Z",
        "producer": "https://example.com/spark",
        "schemaURL": "https://openlineage.io/spec/1-0-5/OpenLineage.json",
        "run": {"runId": "run-1", "facets": {"parent": {"run": {"runId": "run-0"}}}},
        "job": {"namespace": "etl", "name": "load_orders", "facets": {"sql": {"query": "SELECT 1"}}},
        "inputs": [{"namespace": "warehouse", "name": "raw_orders", "facets": {"schema": {"fields": []}}}],
        "outputs": [
            {"namespace": "warehouse", "name": "orders"},
            {"namespace": "warehouse", "name": "orders_summary"}
        ],
        "custom_extension_field": "seen"
    }"""

[<Fact>]
let ``an OpenLineage event's run, job and facets are stored, facets as opaque JSON`` () =
    let row = Evp.openLineageEventRow "acme" now "2" "trace-agent/7" (parse lineageEvent)
    Assert.Equal("acme", row.TenantID)
    Assert.Equal(now, row.ReceivedAt)
    Assert.Equal("COMPLETE", row.EventType)
    Assert.Equal("2026-09-23T10:30:00Z", row.EventTime)
    Assert.Equal(Some now, row.EventTimeParsed)
    Assert.Equal("https://example.com/spark", row.Producer)
    Assert.Equal("https://openlineage.io/spec/1-0-5/OpenLineage.json", row.SchemaURL)
    Assert.Equal("run-1", row.RunID)
    Assert.Equal("""{"parent":{"run":{"runId":"run-0"}}}""", row.RunFacets)
    Assert.Equal("etl", row.JobNamespace)
    Assert.Equal("load_orders", row.JobName)
    Assert.Equal("""{"sql":{"query":"SELECT 1"}}""", row.JobFacets)
    Assert.Equal("2", row.APIVersion)
    Assert.Equal("trace-agent/7", row.Via)
    Assert.Equal("\"seen\"", row.Extra["custom_extension_field"])
    Assert.Equal(1, row.Extra.Count)

// Inputs and outputs are independent lists, each with its own alignment.
[<Fact>]
let ``OpenLineage inputs and outputs are stored as separate aligned arrays, in order`` () =
    let row = Evp.openLineageEventRow "acme" now "" "" (parse lineageEvent)
    Assert.Equal<string[]>([| "warehouse" |], row.InputNamespace)
    Assert.Equal<string[]>([| "raw_orders" |], row.InputName)
    Assert.Equal<string[]>([| """{"schema":{"fields":[]}}""" |], row.InputFacets)
    Assert.Equal<string[]>([| "warehouse"; "warehouse" |], row.OutputNamespace)
    Assert.Equal<string[]>([| "orders"; "orders_summary" |], row.OutputName)
    Assert.Equal<string[]>([| ""; "" |], row.OutputFacets)

[<Fact>]
let ``an OpenLineage event with nothing in it stores empty columns`` () =
    let row = Evp.openLineageEventRow "acme" now "" "" (parse """{"inputs": ["text"], "outputs": {"name": "x"}}""")
    Assert.Equal("", row.EventType)
    Assert.Equal(None, row.EventTimeParsed)
    Assert.Equal("", row.RunFacets)
    // A dataset that is not an object still takes its place.
    Assert.Equal<string[]>([| "" |], row.InputNamespace)
    Assert.Equal<string[]>([| "" |], row.InputName)
    Assert.Equal<string[]>([| "" |], row.InputFacets)
    Assert.Empty row.OutputName
    Assert.True row.Extra.IsEmpty

// The lineage proxy sends the OpenLineage client's Authorization: Bearer,
// one event per request, not an array.
[<Fact>]
let ``a lineage event sent with a Bearer key is stored with the query's api-version and the Via header`` () =
    let headers = [ "Authorization", "Bearer " + Replay.testKey; "Via", "trace-agent 7.83.0" ]
    let response, sink = post "routeDataObs" "/api/v1/lineage?api-version=2" headers (utf8 lineageEvent)
    assertAccepted response

    let write = Assert.Single sink.Writes
    Assert.Equal("storage_openlineage_events", write.Writer)
    let row = Assert.Single(sink.Rows<OpenLineageEventRow>())
    Assert.Equal(Replay.testTenant, row.TenantID)
    Assert.Equal("run-1", row.RunID)
    Assert.Equal("2", row.APIVersion)
    Assert.Equal("trace-agent 7.83.0", row.Via)

[<Fact>]
let ``a lineage request without a key is refused`` () =
    let response, sink = post "routeDataObs" "/api/v1/lineage" [] (utf8 lineageEvent)
    Assert.Equal(403, response.Status)
    Assert.Empty sink.Writes

[<Fact>]
let ``a lineage batch stores every event and keeps an item that is not an object raw`` () =
    let body = utf8 """[{"eventType": "START"}, "text", {"eventType": "COMPLETE"}]"""
    let response, sink = post "routeDataObs" "/api/v1/lineage" withKey body
    assertAccepted response
    Assert.Equal<string list>([ "START"; "COMPLETE" ], sink.Rows<OpenLineageEventRow>() |> List.map _.EventType)

    let raw = Assert.Single(rawPayloads sink)
    Assert.Equal("lineage", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal("\"text\"", Encoding.UTF8.GetString raw.Body)

[<Theory>]
[<InlineData("routeSoftwareInventory", "/api/v2/softinv", "softinv")>]
[<InlineData("routeSynthetics", "/api/v2/synthetics", "synthetics")>]
[<InlineData("routeDataObs", "/api/v1/lineage", "lineage")>]
[<InlineData("routeDataObs", "/api/v2/query-actions", "query-actions")>]
let ``a batched body that is not JSON is kept raw as an unexpected shape`` (routeSet: string, path: string, intake: string) =
    let body = utf8 """[{"a": 1}, """
    let response, sink = post routeSet path withKey body
    assertAccepted response

    let write = Assert.Single sink.Writes
    Assert.Equal("storage_raw_payloads", write.Writer)
    let raw = Assert.Single(rawPayloads sink)
    Assert.Equal(intake, raw.Intake)
    Assert.Equal("unexpected_shape", raw.Reason)
    Assert.Equal("body is not a JSON array or object", raw.Note)
    Assert.Equal<uint8[]>(body, raw.Body)

// Go unmarshals a body of `null` into an empty list of items.
[<Theory>]
[<InlineData("routeSoftwareInventory", "/api/v2/softinv")>]
[<InlineData("routeSynthetics", "/api/v2/synthetics")>]
[<InlineData("routeDataObs", "/api/v1/lineage")>]
[<InlineData("routeDataObs", "/api/v2/query-actions")>]
let ``a batched body of null holds no items`` (routeSet: string, path: string) =
    let response, sink = post routeSet path withKey (utf8 "null")
    assertAccepted response
    Assert.Empty sink.Writes

[<Fact>]
let ``a query action entry is stored whole, with its sorted keys and the sender's origin headers`` () =
    let headers = withKey @ [ "Dd-Evp-Origin", "python-integration"; "Dd-Evp-Origin-Version", "1.2.3" ]
    let body = utf8 """[{"b": 9007199254740993, "a": {"z": 1.50, "y": null}}, {"only": "<tag> & more"}]"""
    let response, sink = post "routeDataObs" "/api/v2/query-actions" headers body
    assertAccepted response

    let rows = sink.Rows<QueryActionResultRow>()
    Assert.Equal(2, rows.Length)
    Assert.Equal("""{"a":{"y":null,"z":1.50},"b":9007199254740993}""", rows[0].Result)
    Assert.Equal<string[]>([| "a"; "b" |], rows[0].Keys)
    Assert.Equal("""{"only":"<tag> & more"}""", rows[1].Result)
    Assert.Equal<string[]>([| "only" |], rows[1].Keys)

    for row in rows do
        Assert.Equal(Replay.testTenant, row.TenantID)
        Assert.Equal("python-integration", row.DDEVPOrigin)
        Assert.Equal("1.2.3", row.DDEVPOriginVersion)

[<Fact>]
let ``a bare query action object is a batch of one, and a repeated key keeps its last value`` () =
    let response, sink = post "routeDataObs" "/api/v2/query-actions" withKey (utf8 """{"a": 1, "a": 2}""")
    assertAccepted response

    let row = Assert.Single(sink.Rows<QueryActionResultRow>())
    Assert.Equal("""{"a":2}""", row.Result)
    Assert.Equal<string[]>([| "a" |], row.Keys)
    Assert.Equal("", row.DDEVPOrigin)

// Go decodes a null item into a nil map and marshals it back as "null".
[<Fact>]
let ``a null query action entry is stored as null with no keys`` () =
    let response, sink = post "routeDataObs" "/api/v2/query-actions" withKey (utf8 "[null]")
    assertAccepted response

    let row = Assert.Single(sink.Rows<QueryActionResultRow>())
    Assert.Equal("null", row.Result)
    Assert.Empty row.Keys

[<Fact>]
let ``every table gives one value per column`` () =
    let check (table: NinjaCat.Api.Storage.Table<'row>) (row: 'row) =
        Assert.Equal(table.Columns.Length, (table.Values row).Length)

    let reportRow, issueRows = Evp.healthReportRows "acme" now reportID (healthReport ()) None
    let discoveryRows = Evp.agentDiscoveryRows "acme" now (discoveryBatch ())
    let softwareRows = Evp.hostSoftwareRows "acme" now (parse softwarePayload)

    let queryAction: QueryActionResultRow =
        { TenantID = "acme"
          ReceivedAt = now
          Result = "{}"
          Keys = [||]
          DDEVPOrigin = ""
          DDEVPOriginVersion = "" }

    check AgentDiscovery.table discoveryRows[0]
    check AgentHealthReports.table reportRow
    check AgentHealthIssues.table issueRows[0]
    check EventManagement.table (eventRow eventEnvelope)
    check HostSoftware.table softwareRows.Head
    check SyntheticsResults.table (Evp.syntheticsResultRow "acme" now (parse syntheticsResult))
    check OpenLineage.table (Evp.openLineageEventRow "acme" now "" "" (parse lineageEvent))
    check QueryActionResults.table queryAction

[<Fact>]
let ``absent values reach the driver as NULL, not as empty text or zero`` () =
    let synthetics = SyntheticsResults.table.Values(Evp.syntheticsResultRow "acme" now (parse "{}"))
    let column (name: string) = synthetics[List.findIndex (fun c -> c = name) SyntheticsResults.table.Columns]
    Assert.Null(column "failure_code")
    Assert.Null(column "netstats_packets_sent")
    Assert.Null(column "netstats_jitter")
    Assert.Null(column "test_started_at_parsed")
    Assert.Equal(box "", column "config")

    let software = Evp.hostSoftwareRows "acme" now (parse """{"host_software": {"software": [{"is_64_bit": true}, {}]}}""")
    let bitness = List.findIndex (fun c -> c = "is_64_bit") HostSoftware.table.Columns
    Assert.Equal(box 1uy, (HostSoftware.table.Values software[0])[bitness])
    Assert.Null((HostSoftware.table.Values software[1])[bitness])
