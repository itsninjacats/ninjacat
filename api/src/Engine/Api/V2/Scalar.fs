/// `POST /api/v2/query/scalar` — the request, from JSON to a parsed
/// timeseries request plus each query's aggregator.
///
/// A scalar request is a timeseries request whose every query also says how
/// its window becomes one value. Everything else — the query language,
/// formulas, functions — is the timeseries engine's, reused as it is.
module NinjaCat.Api.Engine.Api.V2.Scalar

open System.Text.Json
open NinjaCat.Api.Engine.Api.V2.Wire
open NinjaCat.Api.Engine.Api.V2.Timeseries

/// `MetricsAggregator`: how one series' points in the window become one value.
type ScalarAggregator =
    | ScalarAvg
    | ScalarMin
    | ScalarMax
    | ScalarSum
    /// The latest point's value.
    | ScalarLast
    /// sqrt of the sum of squares.
    | ScalarL2norm
    /// Sum of value × bucket width in seconds.
    | ScalarArea

type ParsedScalarRequest =
    { Request: ParsedTimeseriesRequest
      /// Parallel to Request.Queries.
      Aggregators: ScalarAggregator list
      /// A name per output column: the formula as written, or the query's
      /// name when there are no formulas.
      ///
      /// WARNING(undocumented): what Datadog names a formula's column. Its one
      /// recorded response names the column of formula "a" "a"; we use the
      /// formula text for every formula.
      ColumnNames: string list }

/// "mean" is listed beside "avg" in the spec's enum, and means the same.
///
/// WARNING(undocumented): "percentile". It applies to distribution metrics
/// (`p95:`), which are not read yet; it is refused by name.
let private aggregator (i: int) (name: string) : Result<ScalarAggregator, string> =
    match name with
    | "avg"
    | "mean" -> Ok ScalarAvg
    | "min" -> Ok ScalarMin
    | "max" -> Ok ScalarMax
    | "sum" -> Ok ScalarSum
    | "last" -> Ok ScalarLast
    | "l2norm" -> Ok ScalarL2norm
    | "area" -> Ok ScalarArea
    | "percentile" -> Error $"queries[{i}]: aggregator 'percentile' is not supported yet"
    | other -> Error $"queries[{i}]: aggregator must be one of avg, min, max, sum, last, percentile, mean, l2norm, area, got '{other}'"

let decode (json: string) : Result<ScalarFormulaRequestAttributes, string list> =
    try
        match JsonSerializer.Deserialize<ScalarFormulaQueryRequest>(json, NinjaCat.Api.Engine.Serialization.options) with
        | body when isNull (box body) || isNull (box body.Data) || isNull (box body.Data.Attributes) ->
            Error [ "missing field 'data.attributes' in the body" ]
        | body ->
            let a = body.Data.Attributes

            let missing =
                [ if isNull body.Data.Type then "missing field 'type' in data"
                  if a.From.IsNone then "missing field 'from' in data.attributes"
                  if a.To.IsNone then "missing field 'to' in data.attributes"
                  if a.Queries.IsNone then "missing field 'queries' in data.attributes"
                  for q in defaultArg a.Queries [] do
                      if isNull q.DataSource then "missing field 'data_source' in data.attributes.queries[]"
                      if q.DataSource = "metrics" && isNull q.Query then "missing field 'query' in data.attributes.queries[]"
                      if q.DataSource = "metrics" && isNull q.Aggregator then "missing field 'aggregator' in data.attributes.queries[]" ]
                |> List.distinct

            match missing with
            | [] when body.Data.Type = "scalar_request" -> Ok a
            | [] -> Error [ "API input validation failed: Invalid type. Expected \"scalar_request\"." ]
            | errors -> Error errors
    with :? JsonException as e ->
        Error [ $"invalid request body: {e.Message}" ]

let parse (raw: ScalarFormulaRequestAttributes) : Result<ParsedScalarRequest, string list> =
    // The same attributes a timeseries request has, without an interval: the
    // step falls back to Datadog's table for the window.
    let queries = defaultArg raw.Queries []

    let asTimeseries: TimeseriesFormulaRequestAttributes =
        { From = raw.From
          To = raw.To
          Interval = None
          Queries = Some(queries |> List.map (fun q -> { DataSource = q.DataSource; Name = q.Name; Query = q.Query }))
          Formulas = raw.Formulas }

    let aggregators = queries |> List.mapi (fun i q -> if q.DataSource = "metrics" then aggregator i q.Aggregator else Ok ScalarAvg)
    let aggErrors = aggregators |> List.choose (function Error e -> Some e | Ok _ -> None)

    match Timeseries.parse asTimeseries, aggErrors with
    | Ok request, [] ->
        let names =
            match raw.Formulas with
            | Some fs when not fs.IsEmpty -> fs |> List.map _.Formula
            | _ -> queries |> List.mapi (fun i q -> defaultArg q.Name $"query{i + 1}")

        Ok
            { Request = request
              Aggregators = aggregators |> List.choose (function Ok a -> Some a | Error _ -> None)
              ColumnNames = names }
    | Ok _, errors -> Error errors
    | Error errors, more -> Error(errors @ more)

let read (json: string) : Result<ParsedScalarRequest, string list> = decode json |> Result.bind parse
