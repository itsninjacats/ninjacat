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
open Microsoft.Extensions.Logging
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

let private accepted = Response.json 202 "{}"

/// A string as Go's %q prints it.
let private quoted (text: string) : string =
    let out = StringBuilder("\"")

    for c in text do
        match c with
        | '"' -> out.Append "\\\"" |> ignore
        | '\\' -> out.Append "\\\\" |> ignore
        | '\n' -> out.Append "\\n" |> ignore
        | '\r' -> out.Append "\\r" |> ignore
        | '\t' -> out.Append "\\t" |> ignore
        | c when c < ' ' || c = '\127' -> out.Append("\\x").Append((int c).ToString "x2") |> ignore
        | c -> out.Append c |> ignore

    out.Append('"').ToString()

/// One opaque integration payload per request. The agent never looks inside:
/// an integration hands it bytes, and no schema for them is published
/// anywhere. So the bytes are kept as they are, with the sender's identity,
/// which the event platform carries in headers, in the note.
let private handleGenResources (r: Request) : Response =
    let header (name: string) = quoted (r.Header name)

    let note =
        $"""DD-EVP-ORIGIN={header "DD-EVP-ORIGIN"} DD-EVP-ORIGIN-VERSION={header "DD-EVP-ORIGIN-VERSION"} Content-Type={header "Content-Type"}"""

    Raw.store r "genresources" "no_schema" note r.Body
    accepted

let resourcesRoutes: Route list = [ Route.post "/api/v2/genresources" handleGenResources ]

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

/// A value that lands in a column as text: a string as itself, absent and
/// null as "", anything else as its JSON text. Never a "-" placeholder: the
/// columns are not Nullable, and "-" could not be told from a sender's own.
let text (value: JsonElement option) : string =
    match value with
    | None -> ""
    | Some value ->
        match value.ValueKind with
        | JsonValueKind.String -> value.GetString()
        | JsonValueKind.Null -> ""
        | _ -> value.GetRawText()

/// The JSON text exactly as it arrived; "" when absent. Kept next to every
/// parsed field, so a value that does not parse is not lost.
let rawText (value: JsonElement option) : string =
    match value with
    | None -> ""
    | Some value -> value.GetRawText()

/// `seq_id`, read from its digits: a float would round a large sequence
/// number past 2^53.
let seqId (value: JsonElement option) : int64 option =
    match value |> Option.bind GoJson.numberText with
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
                Some(GoTime.fromUnixSeconds Int64.MinValue)
            elif whole >= 253402300799.0 then
                Some(GoTime.fromUnixSeconds Int64.MaxValue)
            else
                let nanos = int64 ((seconds - whole) * 1e9)
                Some((GoTime.fromUnixSeconds (int64 whole)).AddTicks(nanos / 100L))
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
        let fields = value |> Option.map GoJson.members |> Option.defaultValue Map.empty

        Some
            { ServiceName = text (fields.TryFind "service_name")
              ServiceVersion = text (fields.TryFind "service_version")
              Env = text (fields.TryFind "env")
              LanguageName = text (fields.TryFind "language_name")
              LanguageVersion = text (fields.TryFind "language_version")
              TracerVersion = text (fields.TryFind "tracer_version") }

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
        let fields = value |> Option.map GoJson.members |> Option.defaultValue Map.empty

        Some
            { Hostname = text (fields.TryFind "hostname")
              OS = text (fields.TryFind "os")
              Architecture = text (fields.TryFind "architecture")
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
let private requestRow (r: Request) (envelope: Map<string, JsonElement>) (requestType: string) : APMTelemetryRow =
    let app = application (envelope.TryFind "application")
    let sender = host (envelope.TryFind "host")

    // `application` and `host` are known keys, so they are not in `extra`.
    // A block that did not read as an object has no column to go to either,
    // so its text is put back under its own name.
    let extra = extraKeys envelope
    let extra = if app.IsSome then extra else extra.Add("application", rawText (envelope.TryFind "application"))
    let extra = if sender.IsSome then extra else extra.Add("host", rawText (envelope.TryFind "host"))

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

    { TenantID = r.Tenant
      ReceivedAt = DateTime.UtcNow
      RequestType = requestType
      Producer = producer envelope requestType
      APIVersion = text (envelope.TryFind "api_version")
      RuntimeID = text (envelope.TryFind "runtime_id")
      SeqID = seqId (envelope.TryFind "seq_id")
      TracerTime = unixTime (envelope.TryFind "tracer_time")
      TracerTimeRaw = rawText (envelope.TryFind "tracer_time")
      EventTime = unixTime (envelope.TryFind "event_time")
      EventTimeRaw = rawText (envelope.TryFind "event_time")
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
      Payload = rawText (envelope.TryFind "payload")
      Debug = rawText (envelope.TryFind "debug")
      Origin = text (envelope.TryFind "origin")
      Via = r.Header "Via"
      DDAgentHostname = r.Header "DD-Agent-Hostname"
      DDAgentEnv = r.Header "DD-Agent-Env"
      DatadogContainerID = r.Header "Datadog-Container-Id"
      XDatadogContainerTags = r.Header "X-Datadog-Container-Tags"
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
            Some(entries |> List.map GoJson.members)
        else
            None
    | _ -> None

/// One row for the request, whoever sent it. A message-batch, whose payload
/// is [{request_type, payload}, …], adds one row per entry on top of that
/// row, which keeps the whole batch.
let private handleTelemetry (r: Request) : Response =
    let refuse (error: string) =
        r.Log.LogWarning("[apmtelemetry] not a JSON object: {Error}", error)
        Raw.store r "apmtelemetry" "decode_error" error r.Body

    match GoJson.parse r.Body with
    | Error e -> refuse e
    | Ok root when root.ValueKind <> JsonValueKind.Object && root.ValueKind <> JsonValueKind.Null ->
        refuse $"expected a JSON object, got {GoJson.kind root}"
    | Ok root ->
        if r.Tenant <> "" then
            let envelope = GoJson.members root
            let requestType = text (envelope.TryFind "request_type")
            let parent = requestRow r envelope requestType

            let children =
                if requestType <> "message-batch" then
                    []
                else
                    match batchEntries (envelope.TryFind "payload") with
                    | None ->
                        r.Log.LogWarning "[apmtelemetry] batch payload is not a list of objects; kept only on the request's row"
                        []
                    | Some entries ->
                        // Entries carry no envelope of their own: they
                        // inherit the request's identity.
                        entries
                        |> List.mapi (fun i entry ->
                            let entryType = text (entry.TryFind "request_type")

                            { parent with
                                RequestType = entryType
                                Producer = producer envelope entryType
                                Payload = rawText (entry.TryFind "payload")
                                BatchIndex = Some(uint32 i)
                                ParentRequestType = requestType })

            Sink.write r.Sink ApmTelemetry.table (Array.ofList (parent :: children))

    accepted

let telemetryRoutes: Route list = [ Route.post "/api/v2/apmtelemetry" handleTelemetry ]
