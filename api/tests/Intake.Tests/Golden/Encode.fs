/// Turns F# rows into JSON the same way the Go fixture recorder turned Go
/// rows into JSON, so the two can be compared.
module NinjaCat.Api.Intake.Tests.Golden.Encode

open System
open System.Collections
open System.Text
open System.Text.Json.Nodes
open Microsoft.FSharp.Reflection
open NinjaCat.Api.Storage

let private strictUtf8 = UTF8Encoding(false, true)

/// Text as text, anything else as {"$b64": …}.
let private bytes (data: byte[]) : JsonNode =
    try
        JsonValue.Create(strictUtf8.GetString data)
    with :? DecoderFallbackException ->
        let wrapped = JsonObject()
        wrapped["$b64"] <- JsonValue.Create(Convert.ToBase64String data)
        wrapped

/// RFC 3339 with up to nine fraction digits, trailing zeros dropped — Go's
/// time.RFC3339Nano.
let timestamp (seconds: int64) (nanos: int64) : string =
    let whole = DateTimeOffset.FromUnixTimeSeconds(seconds).UtcDateTime.ToString "yyyy-MM-ddTHH:mm:ss"
    let fraction = if nanos = 0L then "" else "." + (nanos.ToString "000000000").TrimEnd '0'
    whole + fraction + "Z"

/// Division rounding down, so times before 1970 keep a positive fraction.
let private floorDiv (a: int64) (b: int64) : int64 =
    let q = a / b
    if a % b < 0L then q - 1L else q

let private dateTime (value: DateTime) : string =
    let ticks = value.ToUniversalTime().Ticks - DateTime.UnixEpoch.Ticks
    let seconds = floorDiv ticks TimeSpan.TicksPerSecond
    timestamp seconds ((ticks - seconds * TimeSpan.TicksPerSecond) * 100L)

let private number (value: float) : JsonNode =
    if Double.IsNaN value then JsonValue.Create "NaN"
    elif Double.IsPositiveInfinity value then JsonValue.Create "+Inf"
    elif Double.IsNegativeInfinity value then JsonValue.Create "-Inf"
    else JsonValue.Create value

let rec value (v: obj) : JsonNode =
    match v with
    | null -> null
    | :? string as s -> JsonValue.Create s
    | :? bool as b -> JsonValue.Create b
    | :? (byte[]) as data -> bytes data
    | :? byte as n -> JsonValue.Create(uint64 n)
    | :? sbyte as n -> JsonValue.Create(int64 n)
    | :? int16 as n -> JsonValue.Create(int64 n)
    | :? uint16 as n -> JsonValue.Create(uint64 n)
    | :? int32 as n -> JsonValue.Create(int64 n)
    | :? uint32 as n -> JsonValue.Create(uint64 n)
    | :? int64 as n -> JsonValue.Create n
    | :? uint64 as n -> JsonValue.Create n
    | :? float32 as n -> number (float n)
    | :? float as n -> number n
    | :? decimal as n -> JsonValue.Create n
    | :? DateTime as t -> JsonValue.Create(dateTime t)
    | :? DateTimeOffset as t -> JsonValue.Create(dateTime t.UtcDateTime)
    | :? Guid as g -> JsonValue.Create(g.ToString "D")
    | :? UnixNanos as n ->
        let (UnixNanos ns) = n
        let seconds = floorDiv ns 1_000_000_000L
        JsonValue.Create(timestamp seconds (ns - seconds * 1_000_000_000L))
    | :? IDictionary as map ->
        let object = JsonObject()

        for entry in map |> Seq.cast<DictionaryEntry> do
            object[string entry.Key] <- value entry.Value

        object
    | other ->
        let t = other.GetType()

        if t.IsGenericType && t.GetGenericTypeDefinition() = typedefof<Map<_, _>> then
            // F# Map is not an IDictionary; read it as key/value pairs.
            let object = JsonObject()

            for pair in other :?> IEnumerable do
                let pairType = pair.GetType()
                object[string (pairType.GetProperty("Key").GetValue pair)] <- value (pairType.GetProperty("Value").GetValue pair)

            object
        elif FSharpType.IsRecord t then
            let object = JsonObject()

            for field in FSharpType.GetRecordFields t do
                object[field.Name] <- value (field.GetValue other)

            object
        elif t.IsGenericType && t.GetGenericTypeDefinition() = typedefof<option<_>> then
            // A boxed None is null and was handled above; this is Some.
            value (t.GetProperty("Value").GetValue other)
        elif FSharpType.IsTuple t then
            JsonArray(FSharpValue.GetTupleFields other |> Array.map value)
        else
            match other with
            | :? IEnumerable as items -> JsonArray(items |> Seq.cast<obj> |> Seq.map value |> Array.ofSeq)
            | _ -> failwith $"Encode.value: no rule for {t.FullName}"
