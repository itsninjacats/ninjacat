/// Two small intakes with one endpoint each.
///
///   resources-intake.<site>                   /api/v2/genresources   opaque bytes
///   instrumentation-telemetry-intake.<site>   /api/v2/apmtelemetry   JSON envelope
///
///   config: none for resources (an event platform track); telemetry is
///           switched off wholesale with DD_TELEMETRY_ENABLED.
module NinjaCat.Api.Intake.Routers.Misc

open System
open System.Globalization
open System.Text
open System.Text.Json
open System.Threading.Tasks
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// One opaque integration payload per request. The agent never looks inside:
/// an integration hands it bytes, and no schema for them is published
/// anywhere. So the bytes are kept as they are, with the sender's identity,
/// which the event platform carries in headers, in the note.
let handleGenResources (body: byte[]) (ctx: HttpContext) : Task =
    Raw.store ctx "genresources" "no_schema" (Raw.originNote ctx) body
    accepted ctx

// instrumentation-telemetry-intake: one path, five producers, one envelope
// with `request_type` as the discriminator.
//
//   tracers, through the trace-agent's proxy: api_version, tracer_time,
//     runtime_id, seq_id, application, host, payload, debug; the proxy adds
//     the Via, DD-Agent-* and container headers.
//   fleet installer: the same plus origin; request_type "logs" or "traces".
//   agent telemetry: event_time instead of tracer_time; request_type
//     agent-metrics, agent-logs, message-batch, or a profile event.
//   trace-agent onboarding, cluster-agent remote config: request_type
//     apm-onboarding-event / apm-remote-config-event and a payload.
//
// None of their types is published, so the envelope is read generically:
// `application` and `host` give their known fields as columns, and `payload`
// stays JSON text whoever sent it.

/// `seq_id`, read from its digits: a float would round a large sequence
/// number past 2^53.
let seqId (value: JsonElement option) : int64 option =
    match value |> Option.bind Json.numberText with
    | None -> None
    | Some digits ->
        match Int64.TryParse(digits, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
        | true, n -> Some n
        | false, _ -> None

/// A telemetry timestamp: Unix seconds, with an optional fraction, sent as a
/// number or (by some producers) as a string. None when absent or not a
/// number, never the epoch.
let unixTime (value: JsonElement option) : DateTime option =
    match value with
    | None -> None
    | Some value ->
        let digits = value.GetRawText().Trim().Trim('"')
        let style = NumberStyles.AllowLeadingSign ||| NumberStyles.AllowDecimalPoint ||| NumberStyles.AllowExponent

        match Double.TryParse(digits, style, CultureInfo.InvariantCulture) with
        | true, seconds when Double.IsFinite seconds ->
            let whole = Math.Truncate seconds

            if whole <= -62135596800.0 then
                Some(Time.fromUnixSeconds Int64.MinValue)
            elif whole >= 253402300799.0 then
                Some(Time.fromUnixSeconds Int64.MaxValue)
            else
                let nanos = int64 ((seconds - whole) * 1e9)
                Some((Time.fromUnixSeconds (int64 whole)).AddTicks(nanos / 100L))
        | _ -> None

type Application =
    { ServiceName: string
      ServiceVersion: string
      Env: string
      LanguageName: string
      LanguageVersion: string
      TracerVersion: string }

/// The `application` block's documented fields; all "" when it is absent.
/// None when it is there but is not an object: then its text must be kept
/// somewhere else, not lost.
let application (value: JsonElement option) : Application option =
    match value with
    | Some value when value.ValueKind <> JsonValueKind.Object && value.ValueKind <> JsonValueKind.Null -> None
    | _ ->
        let fields = value |> Option.map Json.members |> Option.defaultValue Map.empty

        Some
            { ServiceName = Json.text (fields.TryFind "service_name")
              ServiceVersion = Json.text (fields.TryFind "service_version")
              Env = Json.text (fields.TryFind "env")
              LanguageName = Json.text (fields.TryFind "language_name")
              LanguageVersion = Json.text (fields.TryFind "language_version")
              TracerVersion = Json.text (fields.TryFind "tracer_version") }

type Host =
    { Hostname: string
      OS: string
      Architecture: string
      /// The block's other keys, as JSON text by name: gohai and the tracer
      /// both put more there than these three.
      Extra: Map<string, string> }

/// The `host` block; None under the same rule as `application`.
let host (value: JsonElement option) : Host option =
    match value with
    | Some value when value.ValueKind <> JsonValueKind.Object && value.ValueKind <> JsonValueKind.Null -> None
    | _ ->
        let fields = value |> Option.map Json.members |> Option.defaultValue Map.empty

        Some
            { Hostname = Json.text (fields.TryFind "hostname")
              OS = Json.text (fields.TryFind "os")
              Architecture = Json.text (fields.TryFind "architecture")
              Extra =
                fields
                |> Map.filter (fun name _ -> name <> "hostname" && name <> "os" && name <> "architecture")
                |> Map.map (fun _ extra -> extra.GetRawText()) }

/// The envelope keys read by name. Any other key goes to `extra`.
let private envelopeKeys =
    set
        [ "request_type"; "api_version"; "runtime_id"; "seq_id"; "tracer_time"; "event_time"; "application"; "host"
          "payload"; "debug"; "origin" ]

let extraKeys (envelope: Map<string, JsonElement>) : Map<string, string> =
    envelope
    |> Map.filter (fun name _ -> not (envelopeKeys.Contains name))
    |> Map.map (fun _ value -> value.GetRawText())

/// Names the producer from request_type, falling back to the envelope keys
/// that only one producer sets.
let producer (envelope: Map<string, JsonElement>) (requestType: string) : string =
    match requestType with
    | "apm-onboarding-event" -> "trace-agent"
    | "apm-remote-config-event" -> "cluster-agent"
    | "agent-metrics"
    | "agent-logs" -> "agent-telemetry"
    | "app-started"
    | "app-heartbeat"
    | "app-closing"
    | "app-dependencies-loaded"
    | "app-integrations-change"
    | "app-client-configuration-change"
    | "app-product-change"
    | "app-extended-heartbeat"
    | "generate-metrics"
    | "distributions" -> "tracer"
    | _ ->
        // "logs", "traces" and "message-batch" are used by more than one
        // producer; the envelope decides.
        if envelope.ContainsKey "event_time" then "agent-telemetry" // profile events, e.g. agent-bsod
        elif envelope.ContainsKey "origin" then "fleet-installer"
        elif envelope.ContainsKey "runtime_id" then "tracer"
        else "unknown"

/// The row of the request itself, whoever sent it.
let private requestRow (ctx: HttpContext) (envelope: Map<string, JsonElement>) (requestType: string) : APMTelemetryRow =
    let tenant = Ctx.tenant ctx

    let app = application (envelope.TryFind "application")
    let sender = host (envelope.TryFind "host")

    // `application` and `host` are known keys, so they are not in `extra`.
    // A block that did not read as an object has no column to go to either,
    // so its text is put back under its own name.
    let extra = extraKeys envelope
    let extra = if app.IsSome then extra else extra.Add("application", Json.rawOrEmpty (envelope.TryFind "application"))
    let extra = if sender.IsSome then extra else extra.Add("host", Json.rawOrEmpty (envelope.TryFind "host"))

    let app =
        app
        |> Option.defaultValue
            { ServiceName = ""
              ServiceVersion = ""
              Env = ""
              LanguageName = ""
              LanguageVersion = ""
              TracerVersion = "" }

    let sender =
        sender
        |> Option.defaultValue
            { Hostname = ""
              OS = ""
              Architecture = ""
              Extra = Map.empty }

    { TenantID = tenant
      ReceivedAt = DateTime.UtcNow
      RequestType = requestType
      Producer = producer envelope requestType
      APIVersion = Json.text (envelope.TryFind "api_version")
      RuntimeID = Json.text (envelope.TryFind "runtime_id")
      SeqID = seqId (envelope.TryFind "seq_id")
      TracerTime = unixTime (envelope.TryFind "tracer_time")
      TracerTimeRaw = Json.rawOrEmpty (envelope.TryFind "tracer_time")
      EventTime = unixTime (envelope.TryFind "event_time")
      EventTimeRaw = Json.rawOrEmpty (envelope.TryFind "event_time")
      ServiceName = app.ServiceName
      ServiceVersion = app.ServiceVersion
      Env = app.Env
      LanguageName = app.LanguageName
      LanguageVersion = app.LanguageVersion
      TracerVersion = app.TracerVersion
      Hostname = sender.Hostname
      HostOS = sender.OS
      HostArchitecture = sender.Architecture
      HostExtra = sender.Extra
      Payload = Json.rawOrEmpty (envelope.TryFind "payload")
      Debug = Json.rawOrEmpty (envelope.TryFind "debug")
      Origin = Json.text (envelope.TryFind "origin")
      Via = Ctx.header ctx "Via"
      DDAgentHostname = Ctx.header ctx "DD-Agent-Hostname"
      DDAgentEnv = Ctx.header ctx "DD-Agent-Env"
      DatadogContainerID = Ctx.header ctx "Datadog-Container-Id"
      XDatadogContainerTags = Ctx.header ctx "X-Datadog-Container-Tags"
      Extra = extra
      BatchIndex = None
      ParentRequestType = "" }

/// The entries of a message-batch payload, each a {request_type, payload}
/// object; None when the payload is not a list of objects.
let private batchEntries (payload: JsonElement option) : Map<string, JsonElement> list option =
    match payload with
    | Some value when value.ValueKind = JsonValueKind.Null -> Some []
    | Some value when value.ValueKind = JsonValueKind.Array ->
        let entries = List.ofSeq (value.EnumerateArray())

        if entries |> List.forall (fun e -> e.ValueKind = JsonValueKind.Object || e.ValueKind = JsonValueKind.Null) then
            Some(entries |> List.map Json.members)
        else
            None
    | _ -> None

/// One row for the request, whoever sent it. A message-batch, whose payload
/// is [{request_type, payload}, …], adds one row per entry on top of that
/// row, which keeps the whole batch.
let handleTelemetry (body: byte[]) (ctx: HttpContext) : Task =
    let log = Ctx.log ctx

    let refuse (error: string) =
        Raw.store ctx "apmtelemetry" "decode_error" error body

    match Json.tryParse body with
    | Error e -> refuse e
    | Ok root when root.ValueKind <> JsonValueKind.Object && root.ValueKind <> JsonValueKind.Null ->
        refuse $"expected an object, got {Json.kind root}"
    | Ok root ->
        let envelope = Json.members root
        let requestType = Json.text (envelope.TryFind "request_type")
        let parent = requestRow ctx envelope requestType

        let children =
            if requestType <> "message-batch" then
                []
            else
                match batchEntries (envelope.TryFind "payload") with
                | None ->
                    log.LogWarning "[apmtelemetry] batch payload is not a list of objects; kept only on the request's row"
                    []
                | Some entries ->
                    // Entries carry no envelope of their own: they
                    // inherit the request's identity.
                    entries
                    |> List.mapi (fun i entry ->
                        let entryType = Json.text (entry.TryFind "request_type")

                        { parent with
                            RequestType = entryType
                            Producer = producer envelope entryType
                            Payload = Json.rawOrEmpty (entry.TryFind "payload")
                            BatchIndex = Some(uint32 i)
                            ParentRequestType = requestType })

        Ctx.write ctx ApmTelemetry.table (Array.ofList (parent :: children))

    accepted ctx
