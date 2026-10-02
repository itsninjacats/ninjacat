/// Two hosts with nothing in common beyond arriving together.
///
///   webhook-intake.<site>     POST /api/v2/webhook, /api/v2/webhook/
///   intake.synthetics.<site>  GET  /api/unstable/synthetics/agents/tests
///
/// webhook-intake is Datadog's one generic "CI provider webhook" intake. The
/// Jenkins plugin posts here with ?service=<ci instance name> and the headers
/// DD-API-KEY and DD-CI-PROVIDER-NAME: jenkins; Datadog's GitLab documentation
/// gives the same URL for GitLab's webhook integration, whose key arrives as
/// ?dd-api-key= because GitLab's webhook form sets no headers.
///
/// The body is a JSON array of elements batched to 5 MB. One batch MIXES
/// levels: each element says "level": "pipeline" | "stage" | "job", and that
/// field — not the URL, not a header — tells them apart.
///
/// Unverified — GitLab: whether it sends Datadog's level-shaped schema or its
/// own native webhook JSON (object_kind, project, builds…) was not confirmed,
/// so the decoder reads both and keeps every element verbatim.
module NinjaCat.Api.Intake.Routers.CiWebhook

open System
open System.Text
open System.Text.Json
open System.Threading.Tasks
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers.CiVisibility
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// Request headers kept on a webhook row. An allowlist, never a blocklist, so
/// a table kept for 90 days cannot become a place a credential lasts:
/// DD-API-KEY and Authorization are absent on purpose.
let private keptHeaders =
    [ "User-Agent"; "Dd-Ci-Provider-Name"; "Dd-Evp-Origin"; "Dd-Evp-Origin-Version"; "X-Gitlab-Event"
      "X-Gitlab-Event-Uuid"; "X-Gitlab-Instance"; "X-Github-Event"; "X-Github-Delivery"; "X-Github-Hook-Id"
      "X-Jenkins"; "Via" ]

/// The top-level keys of an element with a column of their own; the rest go
/// to `extra`.
let private knownKeys =
    set [ "payload_version"; "level"; "url"; "start"; "end"; "partial_retry"; "queue_time"; "status"; "is_manual"
          "trace_id"; "span_id"; "parent_span_id"; "pipeline_id"; "unique_id"; "name"; "id"; "user"; "parameters"
          "tags"; "node"; "git"; "parent_pipeline"; "pipeline_unique_id"; "pipeline_name"; "stage_id"
          "stage_name"; "parent_stage_id"; "error" ]

/// Which CI system sent an element: what the element says, then the
/// DD-CI-PROVIDER-NAME header, then GitLab's native shape, then "unknown".
/// Never a guess that yields a real provider name: a row tagged "jenkins"
/// that came from elsewhere is worse than one tagged "unknown".
let private provider (element: Value) (headerProvider: string) : string =
    let named = Value.fieldText "provider" element

    if named <> "" then named.ToLowerInvariant()
    elif headerProvider <> "" then headerProvider
    // GitLab's own webhooks carry object_kind; Datadog's schema does not.
    elif Value.has "object_kind" element then "gitlab"
    else "unknown"

/// A JSON bool for a Nullable(UInt8) column: None when the key is absent or
/// not a bool, so "the provider never said" stays apart from false.
let private flag (value: Value) : uint8 option =
    match value with
    | Value.Bool b -> Some(Text.flag b)
    | _ -> None

/// What the request says about every element of its batch.
type Delivery =
    { /// DD-CI-PROVIDER-NAME, lower-cased: only a default, the element's own
      /// `provider` wins.
      Provider: string
      /// The ?service= the Jenkins plugin appends: its CI instance name.
      Service: string
      ID: string
      Headers: Map<string, string> }

/// One element of a batch as a row.
///
/// Every id is read as text even where Jenkins sends a number: the same
/// fields carry UUIDs from other providers. A JSON number keeps its literal,
/// so a numeric id keeps every digit.
let webhookRow (tenant: string) (receivedAt: DateTime) (delivery: Delivery) (body: string) (element: Value) : CIWebhookEventRow =
    let node = Value.field "node" element
    let git = Value.field "git" element
    let user = Value.field "user" element
    let error = Value.field "error" element
    let parentPipeline = Value.field "parent_pipeline" element
    let level = Value.fieldText "level" element
    let startRaw = Value.fieldText "start" element
    let endRaw = Value.fieldText "end" element

    { TenantID = tenant
      ReceivedAt = receivedAt
      Provider = provider element delivery.Provider
      // Datadog's "level" first, then GitLab's native "object_kind".
      Level = (if level <> "" then level else Value.fieldText "object_kind" element)
      Service = delivery.Service
      PayloadVersion = Value.tryInt64 (Value.field "payload_version" element)
      PartialRetry = flag (Value.field "partial_retry" element)
      IsManual = flag (Value.field "is_manual" element)
      TraceID = Value.fieldText "trace_id" element
      SpanID = Value.fieldText "span_id" element
      ParentSpanID = Value.fieldText "parent_span_id" element
      ID = Value.fieldText "id" element
      UniqueID = Value.fieldText "unique_id" element
      PipelineID = Value.fieldText "pipeline_id" element
      PipelineUniqueID = Value.fieldText "pipeline_unique_id" element
      PipelineName = Value.fieldText "pipeline_name" element
      StageID = Value.fieldText "stage_id" element
      StageName = Value.fieldText "stage_name" element
      ParentStageID = Value.fieldText "parent_stage_id" element
      Name = Value.fieldText "name" element
      URL = Value.fieldText "url" element
      Status = Value.fieldText "status" element
      // The text is kept next to the parse: a format we do not recognise
      // loses the index, never the value, and an absent time stays NULL.
      StartRaw = startRaw
      StartParsed = Time.tryRfc3339 startRaw
      EndRaw = endRaw
      EndParsed = Time.tryRfc3339 endRaw
      QueueTimeMs = Value.tryInt64 (Value.field "queue_time" element)
      NodeName = Value.fieldText "name" node
      NodeHostname = Value.fieldText "hostname" node
      NodeWorkspace = Value.fieldText "workspace" node
      NodeLabels = Value.strings (Value.field "labels" node)
      GitRepositoryURL = Value.fieldText "repository_url" git
      GitDefaultBranch = Value.fieldText "default_branch" git
      GitBranch = Value.fieldText "branch" git
      GitSHA = Value.fieldText "sha" git
      GitTag = Value.fieldText "tag" git
      GitMessage = Value.fieldText "message" git
      GitAuthorName = Value.fieldText "author_name" git
      GitAuthorEmail = Value.fieldText "author_email" git
      GitAuthorTime = Value.fieldText "author_time" git
      GitCommitterName = Value.fieldText "committer_name" git
      GitCommitterEmail = Value.fieldText "committer_email" git
      GitCommitTime = Value.fieldText "commit_time" git
      UserName = Value.fieldText "name" user
      UserEmail = Value.fieldText "email" user
      ErrorMessage = Value.fieldText "message" error
      ErrorType = Value.fieldText "type" error
      ErrorDomain = Value.fieldText "domain" error
      ErrorStack = Value.fieldText "stack" error
      ParentPipelineTraceID = Value.fieldText "trace_id" parentPipeline
      ParentPipelineURL = Value.fieldText "url" parentPipeline
      Parameters = Value.stringMap (Value.field "parameters" element)
      // "key:value" strings, and a real multiset: the Jenkins plugin repeats
      // keys for per-configuration axes.
      Tags = Tags.toMultiMap (Value.strings (Value.field "tags" element))
      DeliveryID = delivery.ID
      Headers = delivery.Headers
      Extra = Value.unknown knownKeys element
      Body = body }

let private store (body: byte[]) (ctx: HttpContext) : unit =
    let tenant = Ctx.tenant ctx
    let log = Ctx.log ctx
    let sink = Ctx.sink ctx

    match GoJson.parse body with
    | Error e ->
        log.LogWarning("[ciwebhook] json: {Error} ({Bytes} bytes)", e, body.Length)
        Raw.store ctx "webhook" "decode_error" $"batch: {e}" body
    | Ok root ->
        // A batch is an array; a provider that skipped the batcher sends one
        // object. A bare null is an empty batch.
        let elements =
            match root.ValueKind with
            | JsonValueKind.Array -> List.ofSeq (root.EnumerateArray())
            | JsonValueKind.Null -> []
            | _ -> [ root ]

        let delivery: Delivery =
            { Provider = (Ctx.header ctx "Dd-Ci-Provider-Name").Trim().ToLowerInvariant()
              Service = Ctx.query ctx "service"
              ID = Text.firstNonEmpty [ Ctx.header ctx "X-Gitlab-Event-Uuid"; Ctx.header ctx "X-Github-Delivery"; Ctx.header ctx "Dd-Request-Id" ]
              Headers =
                keptHeaders
                |> List.choose (fun name ->
                    match Ctx.header ctx name with
                    | "" -> None
                    | value -> Some(name, value))
                |> Map.ofList }

        let now = DateTime.UtcNow
        let rows = ResizeArray<CIWebhookEventRow>()

        for written in elements do
            let body = written.GetRawText()

            match Value.ofJson written with
            | Value.Object _ as element ->
                if tenant <> "" then
                    rows.Add(webhookRow tenant now delivery body element)
            | _ -> Raw.store ctx "webhook" "unexpected_shape" "element is not a JSON object" (Encoding.UTF8.GetBytes body)

        Sink.write sink CIWebhookEvents.table (rows.ToArray())

/// A batch of CI pipeline, stage and job events.
///
/// The Jenkins plugin only tells 2xx from everything else, so 202 with an
/// empty object is the whole contract.
let handle (body: byte[]) (ctx: HttpContext) : Task =
    if body.Length > 0 then
        store body ctx

    (setStatusCode 202 >=> json {||}) ctx
/// intake.synthetics.<site> — the agent's synthetics test poller
/// (synthetics.collector.enabled).
///
/// The one GET whose answer DRIVES an agent loop: the agent polls it every
/// two seconds with ?agent_hostname=&agent_version=, anything but 200 counts
/// as a failure, and five failures in a row flip the poller unhealthy. So the
/// answer to "what should I run" while nothing exists is an empty list, not
/// a 404.
///
/// The shape is unforgiving: the whole body is decoded in one go, and one
/// test with a subtype other than "UDP", "TCP" or "ICMP" discards EVERY test
/// in the answer.
///
/// Serving real tests from synthetics_test_configs needs a read path the
/// intake does not have; until then the empty list is the honest answer.
/// Nothing is stored: the poll carries no telemetry.
let handleSyntheticsAgentTests (_: byte[]) : EndpointHandler = json {| tests = List.empty<string> |}

