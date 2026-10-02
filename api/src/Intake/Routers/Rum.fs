/// browser-intake.<site> — Real User Monitoring, from the browser, iOS and
/// Android SDKs.
///
///   config: browser SDK `proxy`, dd-sdk-ios `customEndpoint`,
///           dd-sdk-android `useCustomEndpoint`
///
/// One host for all three, because Datadog has no mobile-intake either: an
/// SDK is redirected by changing a host and nothing else. The platforms
/// differ in how they authenticate and how they signal compression, not in
/// where they send.
///
///   POST /api/v2/rum       NDJSON of RUM events  → rum_views, rum_events,
///                                                  rum_telemetry, rum_timeseries
///   POST /api/v2/logs      NDJSON or JSON array  → logs (Logs.fs)
///   POST /api/v2/replay    multipart             → rum_replay_segments
///   POST /api/v2/spans     NDJSON of envelopes   → rum_spans
///   POST /api/v2/profile   multipart             → (Profiling.fs)
///   GET  /api/v2/profiling/quota                 → nothing; every session is admitted
///   POST /api/v2/debugger  NDJSON or multipart   → (Profiling.fs)
///
/// 202 always. Android drops a batch on 200, iOS on anything but 202, and
/// the browser retries only 408, 429 and 5xx: 202 is the one answer all three
/// read as "kept". Whatever does not decode goes to raw_payloads as "rum".
module NinjaCat.Api.Intake.Routers.Rum

open System
open System.Globalization
open System.IO
open System.Net
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open System.Text.Unicode
open System.Threading.Tasks
open Microsoft.AspNetCore.Cors.Infrastructure
open Microsoft.AspNetCore.Http
open Microsoft.AspNetCore.WebUtilities
open Microsoft.Extensions.Logging
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Net.Http.Headers
open Oxpecker
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// The label of this intake in raw_payloads.
let private intake = "rum"

let private accepted: EndpointHandler = setStatusCode 202 >=> json {||}

/// What a preflight is told it may send. DD-API-KEY and DD-EVP-* are for the
/// mobile SDKs and for a browser deployment that chose headers over the query
/// string; Content-Encoding because a proxy in front of the page may add it.
let private corsAllowedHeaders =
    [| "Content-Type"
       "Content-Encoding"
       "DD-API-KEY"
       "DD-CLIENT-TOKEN"
       "DD-EVP-ORIGIN"
       "DD-EVP-ORIGIN-VERSION"
       "DD-REQUEST-ID"
       "DD-IDEMPOTENCY-KEY" |]

/// The value of NINJACAT_RUM_ALLOWED_ORIGINS as a list: comma-separated
/// origins allowed to post RUM data. Unset, empty or "*" gives no list, which
/// means "any origin".
///
/// Any origin is the default on purpose. The key is compiled into a web page,
/// so an allowlist keeps nobody out, and a mandatory one would turn every new
/// subdomain into silent data loss.
let parseAllowedOrigins (raw: string) : string list =
    if String.IsNullOrEmpty raw || raw.Trim() = "*" then
        []
    else
        raw.Split ',' |> Array.map _.Trim() |> Array.filter (fun origin -> origin <> "") |> List.ofArray

/// The CORS policy of browser-intake, for ASP.NET's CORS middleware.
///
/// The middleware puts the headers on EVERY answer to an allowed origin, the
/// 403 and the 404 included, and that matters: to the browser SDK a
/// cross-origin response it may not read is status 0 while online, which it
/// counts as success. It drops the batch and tells nobody, so a bad key or an
/// unserved path would be invisible on both sides. A preflight carries no
/// credential and is answered by the middleware itself, whatever the path.
let corsPolicy (allowedOrigins: string list) (policy: CorsPolicyBuilder) : unit =
    if allowedOrigins.IsEmpty then
        policy.SetIsOriginAllowed(fun _ -> true) |> ignore
    else
        policy.WithOrigins(Array.ofList allowedOrigins) |> ignore

    policy
        .WithMethods("POST", "GET")
        .WithHeaders(corsAllowedHeaders)
        .SetPreflightMaxAge(TimeSpan.FromDays 1.0)
    |> ignore

/// Turns the browser SDK's `proxy: '<url>'` form back into the request it
/// stands for. That form sends POST <url>?ddforward=<encoded path and query>,
/// with the track, ddsource and the key all inside the one parameter.
///
/// The forwarded query wins on conflict; outer parameters that do not collide
/// are kept, so nothing an intermediary added is thrown away.
let unwrapForward (log: ILogger) (http: HttpContext) : unit =
    let forward = http.Request.Query["ddforward"].ToString()

    if forward <> "" then
        let withoutFragment =
            match forward.IndexOf '#' with
            | -1 -> forward
            | i -> forward.Substring(0, i)

        let path, query =
            match withoutFragment.IndexOf '?' with
            | -1 -> withoutFragment, ""
            | i -> withoutFragment.Substring(0, i), withoutFragment.Substring(i + 1)

        if not (path.StartsWith '/') then
            // Left alone: a 404 naming the path that was actually sent is more
            // use to whoever misconfigured this than a guess would be.
            log.LogWarning("[rum] ddforward={Forward} is not a path+query, routing the request as it came", forward)
        else
            let merged = QueryHelpers.ParseQuery query

            for pair in http.Request.Query do
                let isForward = pair.Key.Equals("ddforward", StringComparison.OrdinalIgnoreCase)

                if not isForward && not (merged.ContainsKey pair.Key) then
                    merged[pair.Key] <- pair.Value

            http.Request.Path <- PathString(Uri.UnescapeDataString path)
            http.Request.QueryString <- QueryString.Create merged

// One path-walking reader over JsonElement serves every event variant: their
// `view` and `session` objects are the same JSON whatever the variant, and a
// field no schema declares yet is read like any other. No number passes
// through a float unless it was written as one.

let private emptyObject: JsonElement = JsonDocument.Parse("{}").RootElement.Clone()

let private jsonText (write: Utf8JsonWriter -> unit) : string =
    use buffer = new MemoryStream()

    do
        use writer = new Utf8JsonWriter(buffer, JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping))
        write writer

    Encoding.UTF8.GetString(buffer.ToArray())

/// JSON from bytes. Go read invalid UTF-8 as U+FFFD; System.Text.Json parses
/// it and then throws when the string is read, so such bytes are repaired
/// before parsing.
let private parseJson (data: byte[]) : Result<JsonElement, string> =
    if Utf8.IsValid(ReadOnlySpan data) then
        Json.tryParse data
    else
        Json.tryParse (Encoding.UTF8.GetBytes(Text.utf8 data))

/// The value at a key path: None when a step is missing or is not an object,
/// and when the value is null.
let private at (path: string list) (root: JsonElement) : JsonElement option =
    let rec walk (current: JsonElement) (rest: string list) =
        match rest with
        | [] -> if current.ValueKind = JsonValueKind.Null then None else Some current
        | key :: tail ->
            if current.ValueKind <> JsonValueKind.Object then
                None
            else
                match current.TryGetProperty key with
                | true, next -> walk next tail
                | false, _ -> None

    walk root path

/// A sub-object, or an empty one, so lookups on a missing branch are empty.
let private objectAt (path: string list) (root: JsonElement) : JsonElement =
    match at path root with
    | Some value when value.ValueKind = JsonValueKind.Object -> value
    | _ -> emptyObject

/// A string field, or "". A value of another type is not coerced: it stays
/// in the event column, visible as what it was.
let private str (path: string list) (root: JsonElement) : string =
    match at path root with
    | Some value when value.ValueKind = JsonValueKind.String -> value.GetString()
    | _ -> ""

/// A value as text: a string as it is, a number with the digits it arrived
/// with, an object or array as JSON.
let private textOf (value: JsonElement) : string =
    match value.ValueKind with
    | JsonValueKind.String -> value.GetString()
    | JsonValueKind.Number -> value.GetRawText()
    | JsonValueKind.True -> "true"
    | JsonValueKind.False -> "false"
    | JsonValueKind.Object
    | JsonValueKind.Array -> jsonText value.WriteTo
    | _ -> ""

/// For fields one SDK writes as text and another as a number: the span ids.
let private text (path: string list) (root: JsonElement) : string =
    match at path root with
    | Some value -> textOf value
    | None -> ""

/// A JSON number as an integer. One written with a fraction or an exponent is
/// truncated rather than lost.
let private integerOf (value: JsonElement) : int64 option =
    match value.TryGetInt64() with
    | true, n -> Some n
    | false, _ ->
        let f = value.GetDouble()
        if Double.IsFinite f then Some(int64 f) else None

/// An integer field; None when absent, so a Nullable column gets NULL and
/// "not measured" never turns into a zero somebody averages.
let private tryInt (path: string list) (root: JsonElement) : int64 option =
    match at path root with
    | Some value when value.ValueKind = JsonValueKind.Number -> integerOf value
    | _ -> None

let private intOrZero (path: string list) (root: JsonElement) : int64 =
    tryInt path root |> Option.defaultValue 0L

let private tryFloat (path: string list) (root: JsonElement) : float option =
    match at path root with
    | Some value when value.ValueKind = JsonValueKind.Number ->
        let f = value.GetDouble()
        if Double.IsFinite f then Some f else None
    | _ -> None

let private tryBool (path: string list) (root: JsonElement) : bool option =
    match at path root with
    | Some value when value.ValueKind = JsonValueKind.True -> Some true
    | Some value when value.ValueKind = JsonValueKind.False -> Some false
    | _ -> None

/// An array of strings in wire order. A lone string counts as a list of one:
/// several SDK fields are documented as arrays and sent as scalars.
let private strings (path: string list) (root: JsonElement) : string[] =
    match at path root with
    | Some value when value.ValueKind = JsonValueKind.Array -> value.EnumerateArray() |> Seq.map textOf |> Array.ofSeq
    | Some value when value.ValueKind = JsonValueKind.String -> [| value.GetString() |]
    | _ -> [||]

let private int64s (path: string list) (root: JsonElement) : int64[] =
    match at path root with
    | Some value when value.ValueKind = JsonValueKind.Array ->
        value.EnumerateArray()
        |> Seq.choose (fun item ->
            if item.ValueKind = JsonValueKind.Number then
                match item.TryGetInt64() with
                | true, n -> Some n
                | false, _ -> None
            else
                None)
        |> Array.ofSeq
    | _ -> [||]

/// A sub-object or array as JSON text for a String column; "" when absent.
let private jsonAt (path: string list) (root: JsonElement) : string =
    match at path root with
    | Some value -> jsonText value.WriteTo
    | None -> ""

let private maxMillis = DateTimeOffset.MaxValue.ToUnixTimeMilliseconds()
let private minMillis = DateTimeOffset.MinValue.ToUnixTimeMilliseconds()

/// DateTime ends at year 9999 and the conversion throws beyond it. A garbage
/// date must not turn the 202 into a 500 the SDK retries forever, so it is
/// clamped.
let private fromMillis (ms: int64) : DateTime =
    Time.fromUnixMillis (max minMillis (min ms maxMillis))

let private tryMillis (path: string list) (root: JsonElement) : DateTime option =
    tryInt path root |> Option.map fromMillis

/// `date` is milliseconds; a missing one is the arrival time, never 1970.
let private wireMillis (ms: int64) : DateTime = Time.wireMillis (min ms maxMillis)

/// Timeseries and span starts are NANOSECONDS, unlike `date`.
let private wireNanos (nanos: int64) : UnixNanos =
    if nanos <= 0L then
        UnixNanos((DateTime.UtcNow - DateTime.UnixEpoch).Ticks * 100L)
    else
        UnixNanos nanos

/// Query parameters that have a column, and both spellings of the
/// credential. Everything else goes to query_extra; these do not, so a key
/// never becomes durable in a table.
let private requestColumns =
    set
        [ "ddsource"; "dd-evp-origin"; "dd-evp-origin-version"; "dd-request-id"; "dd-evp-encoding"
          "batch_time"; "_dd.api"; "_dd.retry_count"; "_dd.retry_after"; "dd-api-key"; "api_key" ]

/// The query parameters without a column as JSON, or "". Values stay arrays:
/// a parameter may legally repeat.
let private queryExtra (ctx: HttpContext) : string =
    let extra =
        ctx.Request.Query
        |> Seq.filter (fun pair -> not (requestColumns.Contains(pair.Key.ToLowerInvariant())))
        |> List.ofSeq

    if extra.IsEmpty then
        ""
    else
        jsonText (fun writer ->
            writer.WriteStartObject()

            for pair in extra do
                writer.WriteStartArray pair.Key

                for value in pair.Value do
                    writer.WriteStringValue value

                writer.WriteEndArray()

            writer.WriteEndObject())

let private tryParseInt64 (text: string) : int64 option =
    match Int64.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
    | true, n -> Some n
    | false, _ -> None

/// The sender's address. Behind a proxy the operator has named
/// (NINJACAT_TRUSTED_PROXIES), ASP.NET's forwarded-headers middleware has
/// already replaced the peer with the address from X-Forwarded-For.
let private clientIP (ctx: HttpContext) : string =
    let peer = ctx.Connection.RemoteIpAddress

    if isNull peer then ""
    elif peer.IsIPv4MappedToIPv6 then peer.MapToIPv4().ToString()
    else peer.ToString()

/// The sender columns shared by every RUM table. The browser puts its
/// identity in the query string (it sets no headers at all), iOS and Android
/// put theirs in headers, so each field is looked for in both places.
let requestInfo (ctx: HttpContext) : RumRequest =
    let tenant = Ctx.tenant ctx

    // The browser spells a retry as _dd.retry_count / _dd.retry_after, iOS
    // and Android as ddtags=retry_count:N,retry_after:CODE. retry_after is
    // stored as sent: a delay in milliseconds from the browser, the HTTP
    // status that caused the retry from the mobile SDKs.
    let retryTags = Tags.toMap (Tags.splitDDTags (Ctx.query ctx "ddtags"))

    let retry (name: string) : int64 option =
        tryParseInt64 (Text.firstNonEmpty [ Ctx.query ctx ("_dd." + name); retryTags.TryFind name |> Option.defaultValue "" ])

    { TenantID = tenant
      ReceivedAt = DateTime.UtcNow
      DDSource = Ctx.query ctx "ddsource"
      EVPOrigin = Text.firstNonEmpty [ Ctx.query ctx "dd-evp-origin"; Ctx.header ctx "Dd-Evp-Origin" ]
      EVPOriginVersion = Text.firstNonEmpty [ Ctx.query ctx "dd-evp-origin-version"; Ctx.header ctx "Dd-Evp-Origin-Version" ]
      // The only record that the batch was compressed: an SDK release that
      // silently stops compressing would otherwise be visible nowhere.
      EVPEncoding = Text.firstNonEmpty [ Ctx.query ctx "dd-evp-encoding"; Ctx.header ctx "Content-Encoding" ]
      RequestID = Text.firstNonEmpty [ Ctx.query ctx "dd-request-id"; Ctx.header ctx "Dd-Request-Id" ]
      IdempotencyKey = Ctx.header ctx "Dd-Idempotency-Key"
      DDAPI = Ctx.query ctx "_dd.api"
      BatchTime = tryParseInt64 (Ctx.query ctx "batch_time") |> Option.map fromMillis
      RetryCount = retry "retry_count" |> Option.filter (fun n -> n >= 0L) |> Option.map uint32
      RetryAfter = retry "retry_after"
      RemoteAddr = clientIP ctx
      UserAgent = Ctx.header ctx "User-Agent"
      QueryExtra = queryExtra ctx }

/// The body, inflated when the browser said so in the query string. The
/// browser sends no Content-Encoding header — ?dd-evp-encoding=deflate is its
/// only signal — so bindBody has passed the body through untouched. A body
/// that does not inflate comes back as it was.
let private inflated (body: byte[]) (ctx: HttpContext) : byte[] =
    let log = Ctx.log ctx

    if body.Length > 0 && (Ctx.query ctx "dd-evp-encoding").Equals("deflate", StringComparison.OrdinalIgnoreCase) then
        Body.decompress log "deflate" body
    else
        body

/// A batch as its non-empty lines. The SDKs write no trailing newline and no
/// \r, but a proxy that rewrote the body might, and a blank line is not an
/// event.
let private ndjsonLines (body: byte[]) : byte[] list =
    let isSpace (b: byte) =
        b = ' 'B || b = '\t'B || b = '\r'B || b = '\n'B || b = 11uy || b = 12uy

    let lines = ResizeArray<byte[]>()
    let mutable lineStart = 0

    for i in 0 .. body.Length do
        if i = body.Length || body[i] = '\n'B then
            let mutable first = lineStart
            let mutable last = i - 1

            while first <= last && isSpace body[first] do
                first <- first + 1

            while last >= first && isSpace body[last] do
                last <- last - 1

            if first <= last then
                lines.Add body[first..last]

            lineStart <- i + 1

    List.ofSeq lines

type EventKind =
    /// "view" or "view_update" → rum_views.
    | View of eventType: string
    /// action, error, resource, long_task, vital, transition → rum_events.
    | Plain of eventType: string
    | Telemetry
    | Timeseries

type private Discriminator =
    | Absent
    | Value of string
    /// Present, but not a string: it can match nothing.
    | NotText

let private discriminator (path: string list) (event: JsonElement) : Discriminator =
    let rec walk (current: JsonElement) (rest: string list) =
        match rest with
        | [] -> if current.ValueKind = JsonValueKind.String then Value(current.GetString()) else NotText
        | key :: tail ->
            if current.ValueKind <> JsonValueKind.Object then
                NotText
            else
                match current.TryGetProperty key with
                | true, next -> walk next tail
                | false, _ -> Absent

    walk event path

/// Which table an event belongs to, decided by the discriminator fields
/// Datadog's schemas declare (`type`, then `vital.type`, `telemetry.type`,
/// `telemetry.status`, `timeseries.name`) and never by shape. The error is
/// the `type` that matched nothing: an SDK newer than this build.
let classify (event: JsonElement) : Result<EventKind, string> =
    let eventType =
        match discriminator [ "type" ] event with
        | Value name -> name
        | _ -> ""

    let unknown = Error eventType

    match eventType with
    | "view"
    | "view_update" -> Ok(View eventType)
    | "action"
    | "transition"
    | "error"
    | "long_task"
    | "resource" -> Ok(Plain eventType)
    | "vital" ->
        match discriminator [ "vital"; "type" ] event with
        | Value("duration" | "operation_step" | "app_launch") -> Ok(Plain eventType)
        | _ -> unknown
    | "telemetry" ->
        let status = discriminator [ "telemetry"; "status" ] event

        match discriminator [ "telemetry"; "type" ] event with
        | Value("configuration" | "usage") -> Ok Telemetry
        // A log event may leave telemetry.type out; its status is what
        // tells an error from a debug message.
        | Absent
        | Value "log" when status = Value "error" || status = Value "debug" -> Ok Telemetry
        | _ -> unknown
    | "timeseries" ->
        match discriminator [ "timeseries"; "name" ] event with
        | Value("memory" | "cpu") -> Ok Timeseries
        | _ -> unknown
    | _ -> unknown

/// A view or view_update as the columns rum_views filters by. Both land in
/// the same row: they carry the same identity and _dd.document_version, and
/// view_update is the newer wire spelling of an update this table already
/// models as a replacement.
let viewRow (request: RumRequest) (eventType: string) (o: JsonElement) (raw: string) : RumViewRow =
    let view = objectAt [ "view" ] o
    let performance = objectAt [ "performance" ] view

    // Web Vitals moved from flat view.* fields into view.performance.* and
    // both spellings are still on the wire: the modern one first, the
    // deprecated one as the fallback.
    let vital (modern: string list) (deprecated: string) : int64 option =
        tryInt modern performance |> Option.orElse (tryInt [ deprecated ] view)

    { Req = request
      Date = wireMillis (intOrZero [ "date" ] o)
      ApplicationID = str [ "application"; "id" ] o
      SessionID = str [ "session"; "id" ] o
      ViewID = str [ "view"; "id" ] o
      DocumentVersion = uint64 (max (intOrZero [ "_dd"; "document_version" ] o) 0L)
      EventType = eventType
      Service = str [ "service" ] o
      Version = str [ "version" ] o
      BuildVersion = str [ "build_version" ] o
      BuildID = str [ "build_id" ] o
      Source = str [ "source" ] o
      SessionType = str [ "session"; "type" ] o
      SessionHasReplay = tryBool [ "session"; "has_replay" ] o
      SessionIsActive = tryBool [ "session"; "is_active" ] o
      SessionSampledForReplay = tryBool [ "session"; "sampled_for_replay" ] o
      UsrID = str [ "usr"; "id" ] o
      UsrName = str [ "usr"; "name" ] o
      UsrEmail = str [ "usr"; "email" ] o
      UsrAnonymousID = str [ "usr"; "anonymous_id" ] o
      AccountID = str [ "account"; "id" ] o
      AccountName = str [ "account"; "name" ] o
      ViewURL = str [ "url" ] view
      ViewName = str [ "name" ] view
      ViewReferrer = str [ "referrer" ] view
      ViewLoadingType = str [ "loading_type" ] view
      ViewLoadingTime = tryInt [ "loading_time" ] view
      ViewTimeSpent = intOrZero [ "time_spent" ] view
      ViewIsActive = tryBool [ "is_active" ] view
      ViewIsSlowRendered = tryBool [ "is_slow_rendered" ] view
      ActionCount = tryInt [ "action"; "count" ] view
      ErrorCount = tryInt [ "error"; "count" ] view
      CrashCount = tryInt [ "crash"; "count" ] view
      LongTaskCount = tryInt [ "long_task"; "count" ] view
      FrozenFrameCount = tryInt [ "frozen_frame"; "count" ] view
      ResourceCount = tryInt [ "resource"; "count" ] view
      FrustrationCount = tryInt [ "frustration"; "count" ] view
      LCP = vital [ "lcp"; "timestamp" ] "largest_contentful_paint"
      CLS =
        tryFloat [ "cls"; "score" ] performance
        |> Option.orElse (tryFloat [ "cumulative_layout_shift" ] view)
      INP = vital [ "inp"; "duration" ] "interaction_to_next_paint"
      FCP = vital [ "fcp"; "timestamp" ] "first_contentful_paint"
      FID = vital [ "fid"; "duration" ] "first_input_delay"
      FBC = tryInt [ "fbc"; "timestamp" ] performance
      TTFB = tryInt [ "first_byte" ] view
      DeviceType = str [ "device"; "type" ] o
      DeviceBrand = str [ "device"; "brand" ] o
      DeviceModel = str [ "device"; "model" ] o
      DeviceName = str [ "device"; "name" ] o
      OSName = str [ "os"; "name" ] o
      OSVersion = str [ "os"; "version" ] o
      ConnectivityStatus = str [ "connectivity"; "status" ] o
      Context = jsonAt [ "context" ] o
      FeatureFlags = jsonAt [ "feature_flags" ] o
      Tags = Tags.toMultiMap (Tags.splitDDTags (str [ "ddtags" ] o))
      Event = raw }

/// An action, error, resource, long_task, vital or transition. Each kind
/// fills its own block of columns and leaves the others empty.
let eventRow (request: RumRequest) (eventType: string) (o: JsonElement) (raw: string) : RumEventRow =
    { Req = request
      Date = wireMillis (intOrZero [ "date" ] o)
      ApplicationID = str [ "application"; "id" ] o
      SessionID = str [ "session"; "id" ] o
      ViewID = str [ "view"; "id" ] o
      EventType = eventType
      Service = str [ "service" ] o
      Version = str [ "version" ] o
      BuildVersion = str [ "build_version" ] o
      BuildID = str [ "build_id" ] o
      Source = str [ "source" ] o
      SessionType = str [ "session"; "type" ] o
      SessionHasReplay = tryBool [ "session"; "has_replay" ] o
      UsrID = str [ "usr"; "id" ] o
      UsrName = str [ "usr"; "name" ] o
      UsrEmail = str [ "usr"; "email" ] o
      UsrAnonymousID = str [ "usr"; "anonymous_id" ] o
      AccountID = str [ "account"; "id" ] o
      AccountName = str [ "account"; "name" ] o
      ViewURL = str [ "view"; "url" ] o
      ViewName = str [ "view"; "name" ] o
      ViewReferrer = str [ "view"; "referrer" ] o
      DeviceType = str [ "device"; "type" ] o
      DeviceBrand = str [ "device"; "brand" ] o
      DeviceModel = str [ "device"; "model" ] o
      DeviceName = str [ "device"; "name" ] o
      OSName = str [ "os"; "name" ] o
      OSVersion = str [ "os"; "version" ] o
      ConnectivityStatus = str [ "connectivity"; "status" ] o
      ActionType = str [ "action"; "type" ] o
      // The label a user sees is action.target.name, not a field on action.
      ActionName = str [ "action"; "target"; "name" ] o
      ActionID = str [ "action"; "id" ] o
      ActionFrustrationTypes = strings [ "action"; "frustration"; "type" ] o
      ErrorID = str [ "error"; "id" ] o
      ErrorMessage = str [ "error"; "message" ] o
      ErrorType = str [ "error"; "type" ] o
      ErrorSource = str [ "error"; "source" ] o
      ErrorStack = str [ "error"; "stack" ] o
      // The crash and ANR reports the mobile SDKs write at the NEXT launch
      // are ordinary errors with this flag set, which is why their date can
      // predate the session they belong to.
      ErrorIsCrash = tryBool [ "error"; "is_crash" ] o
      ErrorHandling = str [ "error"; "handling" ] o
      ErrorFingerprint = str [ "error"; "fingerprint" ] o
      ResourceID = str [ "resource"; "id" ] o
      ResourceType = str [ "resource"; "type" ] o
      ResourceURL = str [ "resource"; "url" ] o
      ResourceMethod = str [ "resource"; "method" ] o
      ResourceStatusCode = tryInt [ "resource"; "status_code" ] o
      ResourceDuration = tryInt [ "resource"; "duration" ] o
      ResourceSize = tryInt [ "resource"; "size" ] o
      LongTaskID = str [ "long_task"; "id" ] o
      LongTaskDuration = tryInt [ "long_task"; "duration" ] o
      VitalID = str [ "vital"; "id" ] o
      VitalType = str [ "vital"; "type" ] o
      VitalName = str [ "vital"; "name" ] o
      VitalDuration = tryInt [ "vital"; "duration" ] o
      Context = jsonAt [ "context" ] o
      Tags = Tags.toMultiMap (Tags.splitDDTags (str [ "ddtags" ] o))
      Event = raw }

/// A type:"telemetry" event: the SDK reporting on itself. Every SDK sends
/// these on the RUM track, including one configured for Logs only.
let telemetryRow (request: RumRequest) (o: JsonElement) (raw: string) : RumTelemetryRow =
    let telemetry = objectAt [ "telemetry" ] o

    { Req = request
      Date = wireMillis (intOrZero [ "date" ] o)
      ApplicationID = str [ "application"; "id" ] o
      SessionID = str [ "session"; "id" ] o
      ViewID = str [ "view"; "id" ] o
      ActionID = str [ "action"; "id" ] o
      Type = str [ "type" ] o
      Status = str [ "status" ] telemetry
      TelemetryType = str [ "type" ] telemetry
      Message = str [ "message" ] telemetry
      ErrorStack = str [ "error"; "stack" ] telemetry
      ErrorKind = str [ "error"; "kind" ] telemetry
      // ~150 optional keys that change with every SDK release: worth
      // keeping whole, not worth a column each.
      Configuration = jsonAt [ "configuration" ] telemetry
      UsageFeature = str [ "usage"; "feature" ] telemetry
      Service = str [ "service" ] o
      Version = str [ "version" ] o
      Source = str [ "source" ] o
      EffectiveSampleRate = tryFloat [ "effective_sample_rate" ] o
      ExperimentalFeatures = strings [ "experimental_features" ] o
      DeviceBrand = str [ "device"; "brand" ] telemetry
      DeviceModel = str [ "device"; "model" ] telemetry
      DeviceArchitecture = str [ "device"; "architecture" ] telemetry
      OSName = str [ "os"; "name" ] telemetry
      OSVersion = str [ "os"; "version" ] telemetry
      OSBuild = str [ "os"; "build" ] telemetry
      Event = raw }

/// A type:"timeseries" event: a mobile-only run of cpu or memory samples,
/// timestamps in one array and the measured quantities in a parallel one.
let timeseriesRow (request: RumRequest) (o: JsonElement) (raw: string) : RumTimeseriesRow =
    let series = objectAt [ "timeseries" ] o

    { Req = request
      Date = wireMillis (intOrZero [ "date" ] o)
      ApplicationID = str [ "application"; "id" ] o
      SessionID = str [ "session"; "id" ] o
      ViewID = str [ "view"; "id" ] o
      Name = str [ "name" ] series
      ID = str [ "id" ] series
      Schema = str [ "schema" ] series
      Start = wireNanos (intOrZero [ "start" ] series)
      End = wireNanos (intOrZero [ "end" ] series)
      Timestamps = int64s [ "data"; "timestamps" ] series
      Values = jsonAt [ "data"; "values" ] series
      Service = str [ "service" ] o
      Version = str [ "version" ] o
      Source = str [ "source" ] o
      Context = jsonAt [ "context" ] o
      Tags = Tags.toMultiMap (Tags.splitDDTags (str [ "ddtags" ] o))
      Event = raw }

/// POST /api/v2/rum: NDJSON, one event per line, from every SDK.
let handleRum (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx
    let sink = Ctx.sink ctx

    if Diagnose.isSweep ctx then
        accepted ctx
    else
        let body = inflated body ctx

        // No tenant means no key, which authentication already refused: nothing is
        // stored, because a guessed tenant puts rows where no query looks.
        if body.Length = 0 || tenant = "" then
            accepted ctx
        else
            let request = requestInfo ctx
            let views = ResizeArray<RumViewRow>()
            let events = ResizeArray<RumEventRow>()
            let telemetry = ResizeArray<RumTelemetryRow>()
            let series = ResizeArray<RumTimeseriesRow>()

            for line in ndjsonLines body do
                match parseJson line with
                | Error problem -> Raw.store ctx intake "decode_error" $"not a JSON object: {problem}" line
                | Ok event when event.ValueKind <> JsonValueKind.Object ->
                    Raw.store ctx intake "decode_error" "not a JSON object" line
                | Ok event ->
                    let raw = Text.utf8 line

                    match classify event with
                    // SDKs ship on schema snapshots months apart, so a newer
                    // one sends event types this build has never seen. That
                    // line is kept raw and the rest of the batch is stored.
                    | Error unknownType -> Raw.store ctx intake "unknown_event" unknownType line
                    | Ok(View eventType) -> views.Add(viewRow request eventType event raw)
                    | Ok(Plain eventType) -> events.Add(eventRow request eventType event raw)
                    | Ok Telemetry -> telemetry.Add(telemetryRow request event raw)
                    | Ok Timeseries -> series.Add(timeseriesRow request event raw)

            Sink.write sink RumViews.table (views.ToArray())
            Sink.write sink RumEvents.table (events.ToArray())
            Sink.write sink RumTelemetry.table (telemetry.ToArray())
            Sink.write sink RumTimeseries.table (series.ToArray())
            accepted ctx

/// A multipart body as its parts, in order and with repeats; an error when
/// the content type is not multipart or the body breaks off.
///
/// Order and repeats both carry meaning here: the mobile canvas-resource
/// variant sends several parts all named "image", and metadata entry i
/// describes blob i.
let private replayParts (contentType: string) (body: byte[]) : Result<MultipartPart list, string> =
    let mediaType = (contentType.Split ';' |> Array.head).Trim().ToLowerInvariant()

    if mediaType = "" then
        Error "mime: no media type"
    elif not (mediaType.StartsWith "multipart/") then
        Error $"content-type is {mediaType}"
    else
        match Multipart.boundary contentType with
        | None -> Error "multipart without boundary"
        | Some boundary ->
            let reader = MultipartReader(boundary, new MemoryStream(body))
            reader.HeadersLengthLimit <- 1024 * 1024
            reader.BodyLengthLimit <- Nullable()
            let parts = ResizeArray<MultipartPart>()

            try
                let mutable section = reader.ReadNextSectionAsync().GetAwaiter().GetResult()

                while not (isNull section) do
                    use data = new MemoryStream()
                    section.Body.CopyTo data

                    let name, fileName =
                        match ContentDispositionHeaderValue.TryParse section.ContentDisposition with
                        | true, disposition ->
                            let file =
                                if disposition.FileNameStar.HasValue then disposition.FileNameStar.Value
                                elif disposition.FileName.HasValue then HeaderUtilities.RemoveQuotes(disposition.FileName).Value
                                else ""

                            HeaderUtilities.RemoveQuotes(disposition.Name).Value |> Option.ofObj |> Option.defaultValue "", file
                        | _ -> "", ""

                    parts.Add
                        { Name = name
                          FileName = fileName
                          ContentType = (if isNull section.ContentType then "" else section.ContentType)
                          Data = data.ToArray() }

                    section <- reader.ReadNextSectionAsync().GetAwaiter().GetResult()

                Ok(List.ofSeq parts)
            with e ->
                Error e.Message

/// The `event` part: one object from the browser, an array from iOS and
/// Android, because a mobile batch spans several views and ships a segment
/// for each. Every entry comes back decoded and as the JSON text stored with
/// its row; None when the part is neither shape.
let replayMetadata (data: byte[]) : (JsonElement * string) list option =
    match parseJson data with
    | Ok root when root.ValueKind = JsonValueKind.Array ->
        root.EnumerateArray()
        |> Seq.map (fun entry ->
            let decoded = if entry.ValueKind = JsonValueKind.Object then entry else emptyObject
            decoded, entry.GetRawText())
        |> List.ofSeq
        |> Some
    | Ok root when root.ValueKind = JsonValueKind.Object -> Some [ root, Text.utf8 data ]
    // Go decoded `null` into an empty object without complaint.
    | Ok root when root.ValueKind = JsonValueKind.Null -> Some [ emptyObject, Text.utf8 data ]
    | _ -> None

/// A recording segment or a canvas resource. The part name is the primary
/// signal ("image" for resources, "segment" or "file<i>" for recordings);
/// the metadata's own "type" is for a producer that names its parts
/// differently.
let replayVariant (partName: string) (meta: JsonElement) : string =
    if partName.StartsWith "image" then "resource"
    elif partName = "segment" || partName.StartsWith "file" then "segment"
    elif str [ "type" ] meta = "resource" then "resource"
    else "segment"

let replayRow (request: RumRequest) (part: MultipartPart) (meta: JsonElement) (metaRaw: string) : RumReplaySegmentRow =
    let variant = replayVariant part.Name meta

    { Req = request
      ApplicationID = str [ "application"; "id" ] meta
      SessionID = str [ "session"; "id" ] meta
      ViewID = str [ "view"; "id" ] meta
      Source = str [ "source" ] meta
      Variant = variant
      PartName = part.Name
      PartFilename = part.FileName
      // None, never the epoch: a resource has no window at all, and 1970
      // would put it on a timeline.
      Start = tryMillis [ "start" ] meta
      End = tryMillis [ "end" ] meta
      RecordsCount = tryInt [ "records_count" ] meta
      HasFullSnapshot = tryBool [ "has_full_snapshot" ] meta
      // The browser sends an index, Android an explicit null, iOS omits the
      // key: three states, so an option.
      IndexInView = tryInt [ "index_in_view" ] meta
      CreationReason = str [ "creation_reason" ] meta
      RawSegmentSize = tryInt [ "raw_segment_size" ] meta
      CompressedSegmentSize = tryInt [ "compressed_segment_size" ] meta
      // The bytes go in the column that says what they are, untouched.
      Segment = (if variant = "resource" then [||] else part.Data)
      Image = (if variant = "resource" then part.Data else [||])
      Event = metaRaw }

/// POST /api/v2/replay: Session Replay uploads, binary parts plus one `event`
/// part of metadata.
///
///   browser segment    part "segment" (zlib) + "event" (one object)
///   mobile segment     parts "file0".."fileN" (zlib) + "event" (an array)
///   resource (canvas)  parts "image" (raw, repeated on mobile) + "event"
///
/// The blobs are not decoded, by design: Datadog's own intake stores segments
/// unprocessed, and the SDK emits zlib streams meant to be concatenated
/// later, which inflating and re-deflating would throw away.
let handleReplay (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx
    let log = Ctx.log ctx
    let sink = Ctx.sink ctx

    if Diagnose.isSweep ctx then
        accepted ctx
    else
        let body = inflated body ctx

        if body.Length = 0 then
            accepted ctx
        else
            match replayParts (Ctx.header ctx "Content-Type") body with
            | Error problem ->
                log.LogWarning("[rum-replay] not multipart ({Problem}), kept raw", problem)
                Raw.store ctx intake "unexpected_shape" $"replay body is not multipart: {problem}" body
                accepted ctx
            | Ok _ when tenant = "" -> accepted ctx
            | Ok parts ->
                let request = requestInfo ctx
                let mutable metadata: (JsonElement * string) list = []

                for part in parts do
                    if part.Name = "event" then
                        match replayMetadata part.Data with
                        | Some entries -> metadata <- entries
                        | None ->
                            metadata <- []
                            Raw.store ctx intake "decode_error" "replay event part is neither an object nor an array" part.Data

                // Entry i describes blob i, in wire order.
                let rows =
                    parts
                    |> List.filter (fun part -> part.Name <> "event")
                    |> List.mapi (fun i part ->
                        match List.tryItem i metadata with
                        | Some(meta, metaRaw) -> replayRow request part meta metaRaw
                        | None -> replayRow request part emptyObject "")

                if not metadata.IsEmpty && rows.Length <> metadata.Length then
                    // Every blob is still stored, but some got the wrong
                    // metadata, and that is worth seeing.
                    log.LogWarning(
                        "[rum-replay] {Blobs} blob parts against {Entries} metadata entries: the pairing is by position and they disagree",
                        rows.Length,
                        metadata.Length
                    )

                Sink.write sink RumReplaySegments.table (Array.ofList rows)
                accepted ctx

/// The span's meta as a flat map with dotted keys. meta.device and meta.os
/// arrive as nested objects among flat keys, and flattening is what keeps
/// meta['device.brand'] reachable. A plain map, not the tag multiset: a JSON
/// object cannot repeat a key.
let flattenMeta (meta: JsonElement) : Map<string, string> =
    let rec flatten (prefix: string) (object: JsonElement) (found: Map<string, string>) : Map<string, string> =
        let mutable result = found

        for property in object.EnumerateObject() do
            let key = if prefix = "" then property.Name else prefix + "." + property.Name

            if property.Value.ValueKind = JsonValueKind.Object then
                result <- flatten key property.Value result
            else
                // A null keeps its key with no value: "sent as null" stays
                // distinct from "not sent".
                result <- result.Add(key, textOf property.Value)

        result

    flatten "" meta Map.empty

let private spanMetrics (metrics: JsonElement) : Map<string, float> =
    metrics.EnumerateObject()
    |> Seq.choose (fun property ->
        if property.Value.ValueKind = JsonValueKind.Number then
            let f = property.Value.GetDouble()
            if Double.IsFinite f then Some(property.Name, f) else None
        else
            None)
    |> Map.ofSeq

let spanRow (request: RumRequest) (env: string) (envelopeExtra: string) (o: JsonElement) (raw: string) : RumSpanRow =
    { Req = request
      // Text, never a number: iOS writes the low 64 bits as hex, Android
      // types all three as strings.
      TraceID = text [ "trace_id" ] o
      SpanID = text [ "span_id" ] o
      ParentID = text [ "parent_id" ] o
      Name = str [ "name" ] o
      Service = str [ "service" ] o
      Resource = str [ "resource" ] o
      Type = str [ "type" ] o
      Env = env
      Start = wireNanos (intOrZero [ "start" ] o)
      DurationNS = intOrZero [ "duration" ] o
      Error = sbyte (intOrZero [ "error" ] o)
      Meta = flattenMeta (objectAt [ "meta" ] o)
      Metrics = spanMetrics (objectAt [ "metrics" ] o)
      Span = raw
      EnvelopeExtra = envelopeExtra }

/// Whatever the envelope carried besides spans and env, as JSON; usually "".
/// A key a future SDK release adds is stored rather than lost.
let private envelopeExtra (envelope: JsonElement) : string =
    let extra =
        envelope.EnumerateObject()
        |> Seq.filter (fun property -> property.Name <> "spans" && property.Name <> "env")
        |> List.ofSeq

    if extra.IsEmpty then
        ""
    else
        jsonText (fun writer ->
            writer.WriteStartObject()

            for property in extra do
                property.WriteTo writer

            writer.WriteEndObject())

/// POST /api/v2/spans: the mobile SDKs' own trace format.
///
/// Not the agent's /v0.4/traces: NDJSON of {"spans":[...],"env":"..."}
/// envelopes with a flat, hand-rolled span shape that only dd-sdk-ios and
/// dd-sdk-android produce, with no published schema.
let handleSpans (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx
    let sink = Ctx.sink ctx

    if Diagnose.isSweep ctx then
        accepted ctx
    else
        let body = inflated body ctx

        if body.Length = 0 || tenant = "" then
            accepted ctx
        else
            let request = requestInfo ctx
            let rows = ResizeArray<RumSpanRow>()

            for line in ndjsonLines body do
                match parseJson line with
                | Error problem -> Raw.store ctx intake "decode_error" problem line
                | Ok envelope when envelope.ValueKind <> JsonValueKind.Object && envelope.ValueKind <> JsonValueKind.Null ->
                    Raw.store ctx intake "decode_error" "span envelope is not an object" line
                | Ok envelope ->
                    // Go decoded a `null` envelope, a `null` spans array and
                    // a `null` span without complaint: as empty.
                    let envelope = if envelope.ValueKind = JsonValueKind.Null then emptyObject else envelope

                    match envelope.TryGetProperty "spans" with
                    | false, _ -> Raw.store ctx intake "unexpected_shape" "span envelope without a spans array" line
                    | true, spans when spans.ValueKind = JsonValueKind.Null -> ()
                    | true, spans when spans.ValueKind <> JsonValueKind.Array ->
                        Raw.store ctx intake "decode_error" "spans is not an array" line
                    | true, spans ->
                        let env = Json.string "env" envelope
                        let extra = envelopeExtra envelope

                        for span in spans.EnumerateArray() do
                            // The span's own text, so it is stored as sent.
                            let raw = span.GetRawText()

                            if span.ValueKind = JsonValueKind.Object then
                                rows.Add(spanRow request env extra span raw)
                            elif span.ValueKind = JsonValueKind.Null then
                                rows.Add(spanRow request env extra emptyObject raw)
                            else
                                Raw.store ctx intake "decode_error" "span is not an object" (Encoding.UTF8.GetBytes raw)

            Sink.write sink RumSpans.table (rows.ToArray())
            accepted ctx

/// GET /api/v2/profiling/quota?session_id=… — the browser profiler asks
/// before it starts whether this session may be profiled. There is no quota
/// here, so every session is admitted. Without an answer the SDK profiles
/// anyway, but marks the session's events with the reason "api-error".
let handleProfilingQuota (_: byte[]) (ctx: HttpContext) : Task =
    json {| data = {| attributes = {| admitted = true; reason = "quota_ok" |} |} |} ctx
