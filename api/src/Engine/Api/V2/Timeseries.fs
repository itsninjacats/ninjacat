/// `POST /api/v2/query/timeseries` — the request, from JSON to parsed trees.
///
/// Two steps, kept apart so each has one kind of error:
///
///   decode   JSON → Wire types                FSharp.SystemTextJson; "missing field 'from'"
///   parse    Wire → ParsedTimeseriesRequest   "queries[0]: expecting '}' at column 23"
///
/// Errors inside query and formula strings are all reported, not just the
/// first, so a client fixing a request does not go round once per mistake.
///
/// The `Parsed…` names are ours on purpose: they are past validation, and must
/// not be mistaken for a schema in Datadog's spec (whose `TimeseriesQuery` is
/// something else — the oneOf of query kinds).
module NinjaCat.Api.Engine.Api.V2.Timeseries

open System
open System.Text.Json
open NinjaCat.Api.Engine.MetricQuery
open NinjaCat.Api.Engine.Api.V2.Wire

// --- what the engine works with --------------------------------------------------

type SortOrder =
    | Asc
    | Desc

/// `limit` on a formula: keep the top or bottom N series. `{order: desc}`
/// without a count was seen in real dashboards, hence the option.
type ParsedLimit = { Count: int option; Order: SortOrder }

type ParsedQuery =
    { Name: string option
      Query: Ast.MetricQuery }

type ParsedFormula =
    { Formula: Ast.Formula
      Limit: ParsedLimit option }

type ParsedTimeseriesRequest =
    { From: DateTimeOffset
      To: DateTimeOffset
      Interval: TimeSpan option
      Queries: ParsedQuery list
      /// Empty means no formulas were sent, and each query is shown as is.
      Formulas: ParsedFormula list }

// --- decode: JSON → Wire -----------------------------------------------

/// Where each Wire record sits in the body, so an error names a JSON location
/// instead of a .NET type the client has never heard of.
let private locations =
    [ nameof TimeseriesFormulaQueryRequest, "the body"
      nameof TimeseriesFormulaRequest, "data"
      nameof TimeseriesFormulaRequestAttributes, "data.attributes"
      nameof MetricsTimeseriesQuery, "data.attributes.queries[]"
      nameof QueryFormula, "data.attributes.formulas[]"
      nameof FormulaLimit, "data.attributes.formulas[].limit" ]

/// Turns a deserializer's message into one about the request body, given where
/// each Wire record sits in it. Shared with the scalar request.
let describeWith (locations: (string * string) list) (message: string) =
    let locate (typeName: string) =
        locations |> List.tryFind (fst >> (=) typeName) |> Option.map snd

    let missing = Text.RegularExpressions.Regex.Match(message, @"^Missing field for record type [\w.+]*\+(\w+): (\w+)")
    let notList = Text.RegularExpressions.Regex.Match(message, @"^Failed to parse type .*FSharpList.*?\+(\w+),.*expected JSON array")

    // "... could not be converted to System.Int64. Path: $ | LineNumber: 0 | ..."
    // — the path is always `$` here, which only misleads; the position does not.
    let converted = Text.RegularExpressions.Regex.Match(message, @"could not be converted to System\.(\w+)\..*(LineNumber: \d+ \| BytePositionInLine: \d+)")

    match () with
    | _ when missing.Success && (locate missing.Groups[1].Value).IsSome ->
        $"missing field '{missing.Groups[2].Value}' in {(locate missing.Groups[1].Value).Value}"
    | _ when notList.Success && (locate notList.Groups[1].Value).IsSome ->
        $"{(locate notList.Groups[1].Value).Value.TrimEnd('[', ']')} must be a JSON array"
    | _ when converted.Success ->
        let expected = if converted.Groups[1].Value.StartsWith "Int" then "an integer" else converted.Groups[1].Value
        $"expected {expected} at {converted.Groups[2].Value}"
    | _ -> message.Replace("Path: $ | ", "")

/// What the deserializer lets through but the spec requires.
let private required (body: TimeseriesFormulaQueryRequest) : string list =
    let missing where name (value: obj) =
        if isNull value then [ $"missing field '{name}' in {where}" ] else []

    let a = body.Data.Attributes

    [ yield! missing "data" "type" body.Data.Type
      if a.From.IsNone then "missing field 'from' in data.attributes"
      if a.To.IsNone then "missing field 'to' in data.attributes"
      for q in a.Queries do
          yield! missing "data.attributes.queries[]" "data_source" q.DataSource
          // Only a metrics query has `query`; the others (logs: `search`, …)
          // are refused by data_source in parse, with a better message.
          if q.DataSource = "metrics" then
              yield! missing "data.attributes.queries[]" "query" q.Query
      for f in defaultArg a.Formulas [] do
          yield! missing "data.attributes.formulas[]" "formula" f.Formula ]
    |> List.distinct

/// Structural errors mostly stop at the first one — that is what the
/// deserializer gives. Errors inside query and formula strings are all
/// collected (parse).
let decode (json: string) : Result<TimeseriesFormulaRequestAttributes, string list> =
    try
        match JsonSerializer.Deserialize<TimeseriesFormulaQueryRequest>(json, NinjaCat.Api.Engine.Json.options) with
        | body when isNull (box body) || isNull (box body.Data) || isNull (box body.Data.Attributes) ->
            Error [ "missing field 'data.attributes' in the body" ]
        | body ->
            match required body with
            | [] when body.Data.Type = "timeseries_request" -> Ok body.Data.Attributes
            // Datadog's own wording, as recorded in its client's tests.
            | [] -> Error [ "API input validation failed: Invalid type. Expected \"timeseries_request\"." ]
            | errors -> Error errors
    with :? JsonException as e ->
        Error [ $"invalid request body: {describeWith locations e.Message}" ]

// --- parse: Wire → ParsedTimeseriesRequest --------------------------------------------------

/// FParsec's message is several lines with a caret under the column; an API
/// error is one line, so keep the position line and what was expected.
let private oneLine (message: string) =
    message.Split('\n', StringSplitOptions.RemoveEmptyEntries ||| StringSplitOptions.TrimEntries)
    |> Array.filter (fun l -> l <> "^" && not (l.StartsWith "Note:"))
    // [ "Error in Ln: 1 Col: 5"; <the input echoed>; "Expecting: ..."; ... ]
    |> fun lines ->
        if lines.Length > 2 then String.Join("; ", Array.append [| lines[0] |] lines[2..])
        else String.Join("; ", lines)

let parse (raw: TimeseriesFormulaRequestAttributes) : Result<ParsedTimeseriesRequest, string list> =
    let errors = ResizeArray<string>()

    let from, to' = raw.From.Value, raw.To.Value

    if from >= to' then
        errors.Add "data.attributes.from must be earlier than data.attributes.to"

    if raw.Queries.IsEmpty then
        errors.Add "data.attributes.queries must not be empty"

    match raw.Interval with
    | Some i when i <= 0L -> errors.Add "data.attributes.interval must be positive"
    | _ -> ()

    let queries =
        raw.Queries
        |> List.mapi (fun i (q: MetricsTimeseriesQuery) ->
            let label = $"""queries[{i}]{q.Name |> Option.map (sprintf " (%s)") |> Option.defaultValue ""}"""

            match q.DataSource with
            | "metrics" ->
                match Parser.parseMetricQuery q.Query with
                | Ok ast -> Some({ Name = q.Name; Query = ast }: ParsedQuery)
                | Error e ->
                    errors.Add $"{label}: {oneLine e}"
                    None
            | other ->
                // The spec's other sources (logs, spans, rum, …) need the search
                // language, which comes later.
                errors.Add $"{label}: data_source '{other}' is not supported yet"
                None)
        |> List.choose id

    let formulas =
        defaultArg raw.Formulas []
        |> List.mapi (fun i (f: QueryFormula) ->
            let label = $"formulas[{i}]"

            let limit =
                f.Limit
                |> Option.bind (fun (l: FormulaLimit) ->
                    match l.Order with
                    | None
                    | Some "desc" -> Some({ Count = l.Count; Order = Desc }: ParsedLimit)
                    | Some "asc" -> Some({ Count = l.Count; Order = Asc }: ParsedLimit)
                    | Some other ->
                        errors.Add $"{label}.limit.order must be 'asc' or 'desc', got '{other}'"
                        None)

            match Parser.parseFormula f.Formula with
            | Ok ast -> Some({ Formula = ast; Limit = limit }: ParsedFormula)
            | Error e ->
                errors.Add $"{label}: {oneLine e}"
                None)
        |> List.choose id

    if errors.Count > 0 then
        Error(List.ofSeq errors)
    else
        Ok
            { From = DateTimeOffset.FromUnixTimeMilliseconds from
              To = DateTimeOffset.FromUnixTimeMilliseconds to'
              Interval = raw.Interval |> Option.map (float >> TimeSpan.FromMilliseconds)
              Queries = queries
              Formulas = formulas }

/// Both steps; the handler needs nothing else.
let read (json: string) : Result<ParsedTimeseriesRequest, string list> = decode json |> Result.bind parse
