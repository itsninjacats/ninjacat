/// http-intake.logs.<site> — the logs intake.
///
///   agent config: logs_config.logs_dd_url / DD_LOGS_CONFIG_LOGS_DD_URL
///
/// One `logs` row per item. An item that is JSON but not shaped like a log
/// goes to raw_payloads as "unexpected_shape", a body that is not JSON as
/// "decode_error".
module NinjaCat.Api.Intake.Routers.Logs

open System
open System.IO
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open Microsoft.Extensions.Logging
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// The fields Datadog's HTTPLogItem declares. Everything else in an item is
/// an attribute.
let private declaredFields = set [ "ddsource"; "ddtags"; "hostname"; "message"; "service" ]

type LogItem =
    { Source: string
      Tags: string
      Hostname: string
      Message: string
      Service: string
      /// The item's other properties, by name.
      Attributes: Map<string, JsonElement> }

type ItemProblem =
    /// JSON, but not a log: a declared field has the wrong type. Carries the
    /// keys that were there.
    | NotALog of keys: string
    | Undecodable of error: string

/// Splits a body into items. Three framings arrive here: a JSON array (the
/// agent), one bare object (small clients) and one object per line (the
/// browser SDK).
let parseItems (body: byte[]) : Result<JsonElement list, string> =
    let firstSignificant =
        body |> Array.tryFind (fun b -> b <> ' 'B && b <> '\t'B && b <> '\r'B && b <> '\n'B)

    if firstSignificant = Some '['B then
        match Json.tryParse body with
        | Ok array -> Ok(List.ofSeq (array.EnumerateArray()))
        | Error e -> Error $"JSON logs array: {e}"
    else
        let items = ResizeArray<JsonElement>()
        let mutable reader = Utf8JsonReader(ReadOnlySpan(Json.repair body), JsonReaderOptions(AllowMultipleValues = true, MaxDepth = Json.maxDepth))

        try
            while reader.Read() do
                items.Add(JsonElement.ParseValue(&reader))

            Ok(List.ofSeq items)
        with e ->
            Error $"JSON logs item {items.Count}: {e.Message}"

/// Reads one item: the five declared fields must be strings (or absent).
/// Datadog's published model calls `message` required, but its intake takes
/// an item without one, and its own .NET tracer sends such items.
let decodeItem (raw: JsonElement) : Result<LogItem, ItemProblem> =
    if raw.ValueKind <> JsonValueKind.Object then
        Error(Undecodable "a log item must be a JSON object")
    else
        let properties = raw.EnumerateObject() |> Seq.map (fun p -> p.Name, p.Value) |> Map.ofSeq

        let declared (name: string) : Result<string option, unit> =
            match properties.TryFind name with
            | None -> Ok None
            | Some value when value.ValueKind = JsonValueKind.Null -> Ok None
            | Some value when value.ValueKind = JsonValueKind.String -> Ok(Some(value.GetString()))
            | Some _ -> Error()

        match declared "ddsource", declared "ddtags", declared "hostname", declared "message", declared "service" with
        | Ok source, Ok tags, Ok hostname, Ok message, Ok service ->
            Ok
                { Source = defaultArg source ""
                  Tags = defaultArg tags ""
                  Hostname = defaultArg hostname ""
                  Message = defaultArg message ""
                  Service = defaultArg service ""
                  Attributes = properties |> Map.filter (fun name _ -> not (declaredFields.Contains name)) }
        | _ -> Error(NotALog(Json.keys raw))

let private attributeString (name: string) (attributes: Map<string, JsonElement>) : string option =
    match attributes.TryFind name with
    | Some value when value.ValueKind = JsonValueKind.String -> Some(value.GetString())
    | _ -> None

/// An integer attribute. A number written with a fraction or an exponent is
/// not one, so a 64-bit id is never rounded through a float.
let private attributeInt64 (name: string) (attributes: Map<string, JsonElement>) : int64 option =
    match attributes.TryFind name with
    | Some value when value.ValueKind = JsonValueKind.Number ->
        match value.TryGetInt64() with
        | true, n -> Some n
        | false, _ -> None
    | _ -> None

/// The row's timestamp, where it came from, and the attribute it used.
///
/// Five wire forms exist for the same idea. The order is Datadog's own:
/// `timestamp` before `date`, a millisecond number before an RFC 3339 string;
/// `@t` is the compact form (see `compact` below). A value that does not
/// parse falls through to the next candidate, and is then left among the
/// attributes rather than lost.
type LogTime =
    { Time: DateTime
      /// "timestamp_ms", "timestamp_string", "date_ms", "date_string",
      /// "compact_string" or "arrival".
      Source: string
      /// The attribute the time was read from; "" when it is the arrival time.
      Key: string }

let timestamp (attributes: Map<string, JsonElement>) (arrival: DateTime) : LogTime =
    let millis (name: string) =
        attributeInt64 name attributes |> Option.filter (fun ms -> ms > 0L) |> Option.map Time.fromUnixMillis

    let text (name: string) =
        attributeString name attributes |> Option.bind Time.tryRfc3339

    match millis "timestamp", text "timestamp", millis "date", text "date", text "@t" with
    | Some t, _, _, _, _ -> { Time = t; Source = "timestamp_ms"; Key = "timestamp" }
    | None, Some t, _, _, _ -> { Time = t; Source = "timestamp_string"; Key = "timestamp" }
    | None, None, Some t, _, _ -> { Time = t; Source = "date_ms"; Key = "date" }
    | None, None, None, Some t, _ -> { Time = t; Source = "date_string"; Key = "date" }
    | None, None, None, None, Some t -> { Time = t; Source = "compact_string"; Key = "@t" }
    // HTTPLogItem has no timestamp field at all, so official clients cannot
    // always send one. The row says so instead of passing arrival off as the
    // sender's clock.
    | None, None, None, None, None -> { Time = arrival; Source = "arrival"; Key = "" }

/// The attributes left after some became columns, as JSON text; "" if none.
let private attributesJson (attributes: Map<string, JsonElement>) : string =
    if attributes.IsEmpty then
        ""
    else
        use buffer = new MemoryStream()

        do
            use writer = new Utf8JsonWriter(buffer, JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping))
            writer.WriteStartObject()

            for pair in attributes do
                writer.WritePropertyName pair.Key
                pair.Value.WriteTo writer

            writer.WriteEndObject()

        Encoding.UTF8.GetString(buffer.ToArray())

/// A .NET log level (Microsoft's names and Serilog's) under the status names
/// the agent and the browser SDK use.
let private compactStatus (level: string) : string =
    match level.ToLowerInvariant() with
    | "trace"
    | "verbose"
    | "debug" -> "debug"
    | "information" -> "info"
    | "warning" -> "warn"
    | "error" -> "error"
    | "critical"
    | "fatal" -> "critical"
    | other -> other

/// What the query string says about the whole batch.
type BatchDefaults =
    { Source: string
      Service: string
      Host: string
      Tags: string list }

let toRow (tenant: string) (item: LogItem) (arrival: DateTime) (defaults: BatchDefaults) : LogRow =
    // `host` is a spelling of `hostname` Datadog also accepts; it is not a
    // declared field, so it arrives as an attribute.
    let hostAttribute =
        if item.Hostname = "" then
            attributeString "host" item.Attributes |> Option.filter (fun h -> h <> "")
        else
            None

    let time = timestamp item.Attributes arrival

    // Datadog's .NET tracer sends logs in Serilog's compact form: @m for the
    // message, @t for the time, @l for the level (left out when it is
    // Information), and dd_service where its log injection already put one.
    // Each stands in only where the usual field is missing.
    let status = attributeString "status" item.Attributes
    let compactMessage = if item.Message = "" then attributeString "@m" item.Attributes else None
    let compactLevel = if status.IsNone then attributeString "@l" item.Attributes else None
    let compactService = if item.Service = "" then attributeString "dd_service" item.Attributes else None

    // Only attributes that were actually USED leave the JSON column.
    let rest =
        item.Attributes
        |> Map.filter (fun name _ ->
            not (
                name = time.Key
                || (name = "status" && status.IsSome)
                || (name = "host" && hostAttribute.IsSome)
                || (name = "@m" && compactMessage.IsSome)
                || (name = "@l" && compactLevel.IsSome)
                || (name = "dd_service" && compactService.IsSome)
            ))

    { TenantID = tenant
      Timestamp = time.Time
      Host = Text.firstNonEmpty [ item.Hostname; defaultArg hostAttribute ""; defaults.Host ]
      Service = Text.firstNonEmpty [ item.Service; defaultArg compactService ""; defaults.Service ]
      Source = Text.firstNonEmpty [ item.Source; defaults.Source ]
      Status =
        match status, compactLevel with
        | Some status, _ -> status
        | None, Some level -> compactStatus level
        | None, None when compactMessage.IsSome -> "info"
        | None, None -> ""
      Message = defaultArg compactMessage item.Message
      // Batch tags first: they are the weaker statement.
      Tags = Tags.toMultiMap (defaults.Tags @ Tags.splitDDTags item.Tags)
      Attributes = attributesJson rest
      TimestampSource = time.Source }

/// Datadog answers logs with an empty object and 202, whatever happened.
let private accepted = Response.json 202 "{}"

let handle (r: Request) : Response =
    if Diagnose.isSweep r || r.Body.Length = 0 then
        accepted
    else
        match parseItems r.Body with
        | Error e ->
            r.Log.LogWarning("[logs] {Error}", e)
            Raw.store r "logs" "decode_error" e r.Body
            accepted
        | Ok items ->
            // Four fields may also arrive as query parameters, so a sender
            // that cannot shape its body can still say what it is. The body
            // wins, except for tags, which merge.
            let defaults =
                { Source = r.Query "ddsource"
                  Service = r.Query "service"
                  Host = Text.firstNonEmpty [ r.Query "hostname"; r.Query "host" ]
                  Tags = Tags.splitDDTags (r.Query "ddtags") }

            let arrival = DateTime.UtcNow
            let rows = ResizeArray<LogRow>()

            items
            |> List.iteri (fun i raw ->
                let rawBytes = Encoding.UTF8.GetBytes(raw.GetRawText())

                match decodeItem raw with
                | Error(NotALog keys) ->
                    r.Log.LogWarning("[logs] item #{Index} is not shaped like a log, kept raw, keys: {Keys}", i, keys)
                    Raw.store r "logs" "unexpected_shape" $"item #{i} did not fit datadogV2.HTTPLogItem" rawBytes
                | Error(Undecodable error) ->
                    r.Log.LogWarning("[logs] item #{Index}: {Error}", i, error)
                    Raw.store r "logs" "decode_error" $"item #{i}: {error}" rawBytes
                // `{}` says nothing: a probe, not a log.
                | Ok item when item.Message = "" && item.Attributes.IsEmpty && item.Source = "" && item.Service = "" -> ()
                | Ok item ->
                    if r.Tenant <> "" then
                        rows.Add(toRow r.Tenant item arrival defaults))

            Sink.write r.Sink Logs.table (rows.ToArray())
            accepted
