namespace NinjaCat.Api.Intake

open System
open System.Text
open System.Text.Json
open System.Text.RegularExpressions
open Google.Protobuf
open Microsoft.Extensions.Logging
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// Datadog's "key:value" tags.
module Tags =
    /// Splits at the FIRST colon: values legitimately contain colons
    /// ("url:http://x"). A bare tag keeps its key with an empty value.
    let split (tag: string) : string * string =
        match tag.IndexOf ':' with
        | -1 -> tag, ""
        | i -> tag.Substring(0, i), tag.Substring(i + 1)

    /// Key → every value seen, in arrival order. Tags are a multiset: two
    /// tags may share a key (kube_service:a and kube_service:b), and both
    /// must survive.
    let toMultiMap (tags: string seq) : Map<string, string[]> =
        let mutable byKey: Map<string, string list> = Map.empty

        for tag in tags do
            let key, value = split tag
            let seen = byKey.TryFind key |> Option.defaultValue []
            byKey <- byKey.Add(key, value :: seen)

        byKey |> Map.map (fun _ values -> values |> List.rev |> Array.ofList)

    /// For "key:value" lists whose keys are unique by contract (Kubernetes
    /// labels and annotations): the last value of a key wins.
    let toMap (pairs: string seq) : Map<string, string> = pairs |> Seq.map split |> Map.ofSeq

    /// `ddtags` is one comma-separated string.
    let splitDDTags (text: string) : string list =
        if String.IsNullOrEmpty text then [] else List.ofArray (text.Split ',')

module Text =
    let firstNonEmpty (values: string list) : string =
        values |> List.tryFind (fun v -> v <> "") |> Option.defaultValue ""

    let utf8 (bytes: byte[]) : string = Encoding.UTF8.GetString bytes

    /// For log lines: the items comma-separated, or "-" when there are none.
    let joinOrDash (items: string seq) : string =
        if Seq.isEmpty items then "-" else String.Join(",", items)

    /// ClickHouse has no Bool in these tables; flags are UInt8.
    let flag (value: bool) : uint8 = if value then 1uy else 0uy

/// Protobuf's variable-length integers, where they appear outside a message
/// the generated classes read: the dictionaries of metrics v3, the DNS
/// buffers of a connections payload, a walk over fields with no schema.
module Varint =
    /// The varint at `position`: its value and the bytes it took. None when
    /// the bytes run out before `stop`, or it is longer than a varint can be.
    let readBefore (data: byte[]) (position: int) (stop: int) : (uint64 * int) option =
        if position < 0 || position >= stop then
            None
        else
            use input = new CodedInputStream(data, position, stop - position)
            let before = input.Position

            try
                let value = input.ReadUInt64()
                Some(value, int (input.Position - before))
            with :? InvalidProtocolBufferException ->
                None

    let read (data: byte[]) (position: int) : (uint64 * int) option = readBefore data position data.Length

/// Protobuf messages as JSON text, written by the protobuf library in
/// proto3's JSON mapping: lowerCamelCase names, 64-bit integers as strings,
/// enums by name, fields at their default left out.
module ProtoJson =
    /// "" for a message that is absent.
    let message (value: IMessage) : string =
        if isNull value then "" else JsonFormatter.Default.Format value

    /// A list of messages as a JSON array; "" for an empty list.
    let messages (values: seq<'message> when 'message :> IMessage) : string =
        if Seq.isEmpty values then
            ""
        else
            "[" + String.Join(",", values |> Seq.map (fun value -> JsonFormatter.Default.Format value)) + "]"

    /// A map of messages as a JSON object, in the map's own order; "" for an
    /// empty map.
    let messageMap (entries: seq<Collections.Generic.KeyValuePair<'key, 'message>> when 'message :> IMessage) : string =
        if Seq.isEmpty entries then
            ""
        else
            let entry (pair: Collections.Generic.KeyValuePair<'key, 'message>) =
                JsonSerializer.Serialize(string pair.Key) + ":" + JsonFormatter.Default.Format pair.Value

            "{" + String.Join(",", entries |> Seq.map entry) + "}"

module Time =
    let private rfc3339 =
        Regex(@"^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?(?:Z|([+-])(\d{2}):(\d{2}))$", RegexOptions.Compiled)

    /// An RFC 3339 timestamp as UTC, or None. It accepts what Go's
    /// time.Parse(time.RFC3339) accepts, since that is what decided which
    /// timestamps were stored so far: a 'T', seconds and a zone are required,
    /// a fraction is optional (after '.' or ','), any offset up to 23:59.
    /// Digits past 100 ns are dropped, not rounded.
    let tryRfc3339 (text: string) : DateTime option =
        let m = rfc3339.Match text

        if not m.Success then
            None
        else
            let number (group: int) = int m.Groups[group].Value
            let year, month, day = number 1, number 2, number 3
            let hour, minute, second = number 4, number 5, number 6
            let offsetHours, offsetMinutes = (if m.Groups[9].Success then number 9 else 0), (if m.Groups[10].Success then number 10 else 0)

            let valid =
                year >= 1
                && month >= 1
                && month <= 12
                && day >= 1
                && day <= DateTime.DaysInMonth(year, month)
                && hour < 24
                && minute < 60
                && second < 60
                && offsetHours < 24
                && offsetMinutes < 60

            if not valid then
                None
            else
                let fraction =
                    if m.Groups[7].Success then
                        int64 (m.Groups[7].Value.PadRight(7, '0').Substring(0, 7))
                    else
                        0L

                let offset = TimeSpan(offsetHours, offsetMinutes, 0).Ticks * (if m.Groups[8].Value = "-" then -1L else 1L)
                let local = DateTime(year, month, day, hour, minute, second, DateTimeKind.Utc).Ticks + fraction
                // A time whose UTC form falls outside years 1 to 9999 is kept at the limit.
                let utc = max DateTime.MinValue.Ticks (min DateTime.MaxValue.Ticks (local - offset))
                Some(DateTime(utc, DateTimeKind.Utc))

    let private maxSeconds = DateTimeOffset.MaxValue.ToUnixTimeSeconds()
    let private minSeconds = DateTimeOffset.MinValue.ToUnixTimeSeconds()

    /// A timestamp past what DateTime holds (year 9999) is kept at that
    /// limit: one absurd value in a payload must not fail the request.
    let fromUnixSeconds (seconds: int64) : DateTime =
        DateTimeOffset.FromUnixTimeSeconds(max minSeconds (min maxSeconds seconds)).UtcDateTime

    let fromUnixMillis (ms: int64) : DateTime =
        DateTimeOffset.FromUnixTimeMilliseconds(max (minSeconds * 1000L) (min (maxSeconds * 1000L + 999L) ms)).UtcDateTime

    /// A timestamp off the wire, in seconds; the receive time when the sender
    /// left it out. Zero does not mean 1970, it means "not supplied" — and a
    /// row stamped 1970 falls out of retention before anyone sees it.
    let wireSeconds (seconds: int64) : DateTime =
        if seconds <= 0L then DateTime.UtcNow else fromUnixSeconds seconds

    /// The same, for milliseconds.
    let wireMillis (millis: int64) : DateTime =
        if millis <= 0L then DateTime.UtcNow else fromUnixMillis millis

    /// A time that may be absent: None unless `present` and positive.
    let optionalSeconds (present: bool) (seconds: int64) : DateTime option =
        if present && seconds > 0L then Some(fromUnixSeconds seconds) else None

module Json =
    // A valid surrogate pair, half of one, or any other escape. Matching
    // every escape is what keeps `\\ud800` (a backslash, then text) apart.
    let private escapes =
        Regex(
            @"\\u[dD][89abAB][0-9a-fA-F]{2}\\u[dD][c-fC-F][0-9a-fA-F]{2}|\\u[dD][89a-fA-F][0-9a-fA-F]{2}|\\.",
            RegexOptions.Compiled ||| RegexOptions.Singleline
        )

    /// Go read a string holding bytes that are not UTF-8, or half of a
    /// surrogate pair (Python writes `\udc80` for a byte it could not
    /// decode), with U+FFFD in their place. System.Text.Json parses such a
    /// body and then throws when the string is read, which would turn one
    /// odd character into a 500. So a body is repaired before it is parsed.
    let repair (body: byte[]) : byte[] =
        let span = ReadOnlySpan body
        let mayHoldHalfPair = span.IndexOf(ReadOnlySpan "\\ud"B) >= 0 || span.IndexOf(ReadOnlySpan "\\uD"B) >= 0

        if System.Text.Unicode.Utf8.IsValid span && not mayHoldHalfPair then
            body
        else
            let text = Encoding.UTF8.GetString body
            // Of the three alternatives only half a pair is six characters long.
            Encoding.UTF8.GetBytes(escapes.Replace(text, (fun found -> if found.Length = 6 then "\\ufffd" else found.Value)))

    /// How deep a JSON body may nest, for every parser of the intake.
    ///
    /// A limit has to exist: several functions here walk a document by
    /// recursion, and a stack overflow in .NET ends the process. It also has
    /// to stay below what Utf8JsonWriter will write back (1000), or a body
    /// that parsed would throw when a column is written. No telemetry nests
    /// anywhere near this; a body that does is kept in raw_payloads.
    let maxDepth = 512

    let private documentOptions = JsonDocumentOptions(MaxDepth = maxDepth)

    /// The body as JSON, or the parser's message.
    let tryParse (body: byte[]) : Result<JsonElement, string> =
        try
            use doc = JsonDocument.Parse(ReadOnlyMemory(repair body), documentOptions)
            Ok(doc.RootElement.Clone())
        with e ->
            Error e.Message

    /// The property as a string: None when it is missing or not a string.
    let tryString (name: string) (object: JsonElement) : string option =
        match object.TryGetProperty name with
        | true, value when value.ValueKind = JsonValueKind.String -> Some(value.GetString())
        | _ -> None

    /// The property as a string, or "".
    let string (name: string) (object: JsonElement) : string =
        tryString name object |> Option.defaultValue ""

    /// The property as an integer: None when it is missing or not a number.
    /// A number with a fraction is truncated, as Go's int64(float64) does.
    let tryInt64 (name: string) (object: JsonElement) : int64 option =
        match object.TryGetProperty name with
        | true, value when value.ValueKind = JsonValueKind.Number ->
            match value.TryGetInt64() with
            | true, n -> Some n
            | false, _ -> Some(int64 (value.GetDouble()))
        | _ -> None

    /// A body that may be one object or an array of them, as a list.
    let tryParseList (body: byte[]) : Result<JsonElement list, string> =
        match tryParse body with
        | Error e -> Error e
        | Ok root when root.ValueKind = JsonValueKind.Array -> Ok(List.ofSeq (root.EnumerateArray()))
        // `null` is no items, not one empty item.
        | Ok root when root.ValueKind = JsonValueKind.Null -> Ok []
        | Ok root -> Ok [ root ]

    /// The object's property names, sorted and comma-separated, for a log
    /// line or a note that says what was there.
    let keys (object: JsonElement) : string =
        if object.ValueKind <> JsonValueKind.Object then
            "(none)"
        else
            let names = object.EnumerateObject() |> Seq.map _.Name |> Seq.sort |> List.ofSeq
            if names.IsEmpty then "(none)" else String.Join(", ", names)

/// Small conversions shared by the wire formats.
module Wire =
    /// A metric type number as its name. Zero is UNSPECIFIED and not rare:
    /// Datadog's own documentation example sends it. It is stored as sent;
    /// resolving it is a decision for query time.
    let metricTypeName (value: int) : string =
        match value with
        | 1 -> "COUNT"
        | 2 -> "RATE"
        | 3 -> "GAUGE"
        | _ -> "UNSPECIFIED"

    /// Node counts per version, as the unsigned numbers the column holds. A
    /// negative count cannot be real and would wrap to billions if cast.
    let versionSpread (counts: seq<string * int>) : Map<string, uint32> =
        counts |> Seq.map (fun (version, nodes) -> version, uint32 (max nodes 0)) |> Map.ofSeq

module Raw =
    /// Request headers kept with a raw payload. An allowlist: a blocklist is
    /// one forgotten header away from writing Dd-Api-Key into a table. These
    /// identify the sender; none is a secret.
    let private keptHeaders =
        [ "Via"; "User-Agent"; "Dd-Evp-Origin"; "Dd-Evp-Origin-Version"; "Dd-Request-Id"
          "Dd-Agent-Hostname"; "Dd-Agent-Env"; "Datadog-Container-Id"; "X-Datadog-Additional-Tags"
          "X-Datadog-Container-Tags"; "X-Requested-With"; "X-Dd-Hostname"; "X-Dd-Processagentversion"
          "X-Dd-Request-Id" ]

    /// The agent probes every intake at startup with nothing, `{}` or `[]`.
    let isProbe (body: byte[]) : bool =
        let text = (Text.utf8 body).Trim()
        text = "" || text = "{}" || text = "[]"

    /// Keeps a request that could not be turned into rows.
    ///
    /// The rule of this project: a handler that cannot decode a body, or gets
    /// a shape it has no schema for, hands the bytes here. A payload that
    /// would show what an intake sends arrives once, from someone's real
    /// agent; keeping it turns reverse-engineering into a SELECT. Bodies that
    /// did become rows are never stored here as well.
    ///
    ///   intake  the endpoint family ("logs", "trace", "rum")
    ///   reason  "no_schema" | "decode_error" | "unexpected_shape"
    ///   note    the decoder's error, the field that was missing
    ///   body    what the handler tried to decode, already decompressed
    let store (r: Request) (intake: string) (reason: string) (note: string) (body: byte[]) : unit =
        if r.Tenant = "" then
            r.Log.LogWarning("[raw] {Intake} {Path}: no tenant, payload dropped ({Bytes} B)", intake, r.Path, body.Length)
        elif isProbe body then
            r.Log.LogDebug("[raw] {Intake} {Path}: empty probe body, not stored", intake, r.Path)
        else
            let headers =
                keptHeaders
                |> List.choose (fun name ->
                    match r.Header name with
                    | "" -> None
                    | value -> Some(name, value))
                |> Map.ofList

            let query =
                r.Http.Request.Query
                |> Seq.map (fun pair -> pair.Key, pair.Value.ToArray())
                |> Map.ofSeq

            let row: RawPayloadRow =
                { TenantID = r.Tenant
                  ReceivedAt = DateTime.UtcNow
                  Intake = intake
                  Reason = reason
                  Method = r.Method
                  // The Host header is what the whole intake dispatches on.
                  Host = r.Host
                  Path = r.Path
                  Query = query
                  ContentType = r.Header "Content-Type"
                  ContentEncoding = r.Header "Content-Encoding"
                  Headers = headers
                  Body = body
                  BodyBytes = uint64 body.Length
                  Note = note }

            Sink.write r.Sink RawPayloads.table [| row |]

/// Counters the intake keeps about itself, stored as ordinary metrics
/// (`ninjacat.intake.<name>`) so they can be graphed like any other.
module SelfMetrics =
    let imagesSkipped = "images.skipped"

    let count (r: Request) (host: string) (name: string) (value: float) (tags: Map<string, string[]>) : unit =
        if value <> 0.0 then
            let point =
                { Metrics.point r.Tenant DateTime.UtcNow ("ninjacat.intake." + name) value with
                    Host = host
                    MetricType = "COUNT"
                    SourceType = "ninjacat"
                    Tags = tags }

            Sink.write r.Sink Metrics.table [| point |]

module Diagnose =
    /// The agent's connectivity sweep at startup: an empty body or `{}`,
    /// nothing to decode.
    let isSweep (r: Request) : bool =
        r.Header "X-Requested-With" = "datadog-agent-diagnose"
