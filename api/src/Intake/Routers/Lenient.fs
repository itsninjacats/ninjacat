/// Input read as leniently as the Go intake read it, for Profiling.fs and
/// Security.fs.
///
/// Go's JSON decoder turns what is not valid text — bytes that are not
/// UTF-8, half a surrogate pair written as an escape — into U+FFFD and
/// carries on. System.Text.Json parses such a document and then throws when
/// the string is read. Producers do send it: a tracer that cuts a long
/// string in the middle of an emoji leaves half a pair behind.
module NinjaCat.Api.Intake.Routers.Lenient

open System
open System.Text
open System.Text.Json
open System.Text.RegularExpressions
open System.Text.Unicode
open NinjaCat.Api.Intake

/// .NET's default is 64, which a symbol database's nested scopes could reach.
let jsonDepth = Json.maxDepth

/// One JSON value with nothing but white space around it, or the parser's
/// message. Bytes that are not UTF-8 are replaced first.
let tryJson (data: byte[]) : Result<JsonElement, string> =
    let readable =
        if Utf8.IsValid(ReadOnlySpan data) then data else Encoding.UTF8.GetBytes(Encoding.UTF8.GetString data)

    try
        use document = JsonDocument.Parse(ReadOnlyMemory readable, JsonDocumentOptions(MaxDepth = jsonDepth))
        Ok(document.RootElement.Clone())
    with :? JsonException as e ->
        Error e.Message

/// The same, when only an object will do.
let tryObject (data: byte[]) : JsonElement option =
    match tryJson data with
    | Ok value when value.ValueKind = JsonValueKind.Object -> Some value
    | _ -> None

/// The text of a JSON string given as it was written, quotes included.
let private unescape (written: string) : string =
    let unescaped = StringBuilder()
    let mutable i = 1

    while i < written.Length - 1 do
        if written[i] <> '\\' then
            unescaped.Append written[i] |> ignore
            i <- i + 1
        elif written[i + 1] = 'u' then
            unescaped.Append(char (Convert.ToInt32(written.Substring(i + 2, 4), 16))) |> ignore
            i <- i + 6
        else
            let escaped =
                match written[i + 1] with
                | 'n' -> '\n'
                | 't' -> '\t'
                | 'r' -> '\r'
                | 'b' -> '\b'
                | 'f' -> '\f'
                | other -> other

            unescaped.Append escaped |> ignore
            i <- i + 2

    // Encoding and decoding turns half a surrogate pair into U+FFFD.
    Encoding.UTF8.GetString(Encoding.UTF8.GetBytes(unescaped.ToString()))

/// The text of a JSON string.
let text (value: JsonElement) : string =
    try
        value.GetString()
    with :? InvalidOperationException ->
        unescape (value.GetRawText())

/// The name of a member of a JSON object.
let nameOf (property: JsonProperty) : string =
    try
        property.Name
    with :? InvalidOperationException ->
        // The member as written starts with its quoted name; an escaped
        // quote does not end it.
        let written = property.ToString()
        let mutable close = 1

        while written[close] <> '"' do
            close <- close + (if written[close] = '\\' then 2 else 1)

        unescape (written.Substring(0, close + 1))

/// A member of a JSON object: the last one of that name, as Go keeps it.
/// None when the object has none.
let property (name: string) (object: JsonElement) : JsonElement option =
    try
        match object.TryGetProperty name with
        | true, value -> Some value
        | false, _ -> None
    with :? InvalidOperationException ->
        // The built-in search stops at a member whose own name is not text.
        object.EnumerateObject()
        |> Seq.filter (fun p -> nameOf p = name)
        |> Seq.tryLast
        |> Option.map _.Value

/// RFC 3339 text as a time, read as Go's time.Parse reads it: the hour may
/// have one digit, a comma may stand before the fraction, and digits past
/// the seventh are cut off, where the shared parser would round them up.
let rfc3339 (written: string) : DateTime option =
    let withHour = Regex.Replace(written, @"T(\d):", "T0$1:")
    Time.tryRfc3339 (Regex.Replace(withHour, @"[.,](\d{1,7})\d*", ".$1"))
