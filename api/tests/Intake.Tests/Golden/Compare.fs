/// Compares what Go produced with what F# produced, as JSON.
///
/// Equal means equal in meaning: object key order does not matter, a number
/// is a number however it is written, a timestamp may lose digits past
/// 100 ns (all a DateTime holds), and a string that is itself JSON is
/// compared as JSON.
module NinjaCat.Api.Intake.Tests.Golden.Compare

open System
open System.Globalization
open System.Text.Json
open System.Text.Json.Nodes
open System.Text.RegularExpressions

let private rfc3339 =
    Regex(@"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(\.\d+)?Z$", RegexOptions.Compiled)

/// A timestamp cut to 100 ns, or None when the text is not one.
let private timestampTo100ns (text: string) : string option =
    let m = rfc3339.Match text

    if not m.Success then
        None
    else
        let digits = if m.Groups[2].Success then m.Groups[2].Value.Substring 1 else ""
        Some(m.Groups[1].Value + "." + digits.PadRight(7, '0').Substring(0, 7))

let private sameNumber (expected: string) (actual: string) : bool =
    if expected = actual then
        true
    else
        match
            Decimal.TryParse(expected, NumberStyles.Float, CultureInfo.InvariantCulture),
            Decimal.TryParse(actual, NumberStyles.Float, CultureInfo.InvariantCulture)
        with
        | (true, a), (true, b) -> a = b
        | _ ->
            let a = Double.Parse(expected, CultureInfo.InvariantCulture)
            let b = Double.Parse(actual, CultureInfo.InvariantCulture)
            a = b || abs (a - b) <= 1e-12 * max (abs a) (abs b)

let private tryJsonContainer (text: string) : JsonNode option =
    let trimmed = text.TrimStart()

    if trimmed.StartsWith '{' || trimmed.StartsWith '[' then
        try
            Some(JsonNode.Parse text)
        with _ ->
            None
    else
        None

let private kind (node: JsonNode) : JsonValueKind =
    if isNull node then JsonValueKind.Null else node.GetValueKind()

let private show (node: JsonNode) : string =
    let text = if isNull node then "null" else node.ToJsonString()
    if text.Length > 160 then text.Substring(0, 160) + "…" else text

/// Every difference between `expected` (Go) and `actual` (F#), as readable
/// lines. `ignored` are paths not compared: volatile values and overrides.
let differences (ignored: Set<string>) (root: string) (expected: JsonNode) (actual: JsonNode) : string list =
    let found = ResizeArray<string>()

    let isIgnored (path: string) =
        ignored.Contains path || ignored.Contains(path + "/$b64")

    let rec walk (path: string) (expected: JsonNode) (actual: JsonNode) =
        if isIgnored path then
            ()
        else
            match kind expected, kind actual with
            | JsonValueKind.Object, JsonValueKind.Object ->
                let e = expected.AsObject()
                let a = actual.AsObject()

                for pair in e do
                    if a.ContainsKey pair.Key then
                        walk $"{path}/{pair.Key}" pair.Value a[pair.Key]
                    elif not (isIgnored $"{path}/{pair.Key}") then
                        found.Add $"{path}/{pair.Key}: missing, Go has {show pair.Value}"

                for pair in a do
                    if not (e.ContainsKey pair.Key) && not (isIgnored $"{path}/{pair.Key}") then
                        found.Add $"{path}/{pair.Key}: not in Go, F# has {show pair.Value}"
            | JsonValueKind.Array, JsonValueKind.Array ->
                let e = expected.AsArray()
                let a = actual.AsArray()

                if e.Count <> a.Count then
                    found.Add $"{path}: Go has {e.Count} items, F# has {a.Count}"
                else
                    for i in 0 .. e.Count - 1 do
                        walk $"{path}/{i}" e[i] a[i]
            | JsonValueKind.Number, JsonValueKind.Number ->
                if not (sameNumber (expected.ToJsonString()) (actual.ToJsonString())) then
                    found.Add $"{path}: Go {show expected}, F# {show actual}"
            | JsonValueKind.String, JsonValueKind.String ->
                let e = expected.GetValue<string>()
                let a = actual.GetValue<string>()

                if e <> a then
                    match timestampTo100ns e, timestampTo100ns a with
                    | Some et, Some at when et = at -> ()
                    | _ ->
                        match tryJsonContainer e, tryJsonContainer a with
                        | Some ej, Some aj -> walk (path + "~json") ej aj
                        | _ -> found.Add $"{path}: Go {show expected}, F# {show actual}"
            | (JsonValueKind.True | JsonValueKind.False), (JsonValueKind.True | JsonValueKind.False) ->
                if expected.GetValue<bool>() <> actual.GetValue<bool>() then
                    found.Add $"{path}: Go {show expected}, F# {show actual}"
            | JsonValueKind.Null, JsonValueKind.Null -> ()
            | _ -> found.Add $"{path}: Go {show expected}, F# {show actual}"

    walk root expected actual
    List.ofSeq found
