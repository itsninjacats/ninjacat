/// The query engine as F# code calls it — monitors, jobs, anything that is
/// not an HTTP client.
///
/// Queries and formulas go in as the same strings Datadog takes; results come
/// back as plain records, with no JSON shape, parallel lists or "key:value"
/// strings to take apart. The HTTP endpoints run the same engine and turn its
/// results into Datadog's response shape; this module turns them into these.
///
///   let! result =
///       Engine.timeseries execute tenant
///           { From = now.AddHours -1.0; To = now; Interval = None
///             Queries = [ { Name = "cpu"; Query = "avg:system.cpu.user{env:prod} by {host}" } ]
///             Formulas = [] }
///
///   match result with
///   | Ok [ cpu ] -> for s in cpu.Series do printfn "%A: %d points" s.Tags s.Points.Length
///   | Error problems -> …
module NinjaCat.Api.Engine.Query.Engine

open System
open System.Threading.Tasks
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Api.V2
open NinjaCat.Api.Engine.Api.V2.Scalar
open NinjaCat.Api.Engine.MetricQuery
open NinjaCat.Api.Engine.MetricQuery.Ast

// --- what goes in ---------------------------------------------------------------------

/// One query, as Datadog writes it: `avg:system.cpu.user{env:prod} by {host}`.
type Query = { Name: string; Query: string }

type TimeseriesRequest =
    { From: DateTimeOffset
      To: DateTimeOffset
      /// A hint for the bucket width; None picks it from the window, as
      /// Datadog does.
      Interval: TimeSpan option
      Queries: Query list
      /// Formulas over the queries' names — `a / b * 100`, `top(a, 5, 'mean',
      /// 'desc')`. Empty: each query is a result of its own.
      Formulas: string list }

/// A query for a scalar: the window reduced to one value per group.
type ScalarQuery =
    { Name: string
      Query: string
      Aggregator: ScalarAggregator }

type ScalarRequest =
    { From: DateTimeOffset
      To: DateTimeOffset
      Queries: ScalarQuery list
      Formulas: string list }

// --- what comes out -------------------------------------------------------------------

/// One `by` key of a series' group. Value None: the series has no such tag —
/// Datadog's N/A group.
type Tag = { Key: string; Value: string option }

type Band = { Lower: float; Upper: float }

type Point =
    { Time: DateTimeOffset
      Value: float
      /// anomalies(): the expected range at this point, where known.
      Expected: Band option }

type ForecastPoint =
    { Time: DateTimeOffset
      Value: float
      Band: Band }

/// One group's series: its tags, and its points in time order. A bucket with
/// no value is simply absent.
type TimeSeries =
    { Tags: Tag list
      Points: Point list
      /// forecast(): what comes after the window. Empty otherwise.
      Forecast: ForecastPoint list }

/// One formula (or query, when there are no formulas) and its series.
type FormulaResult =
    { Formula: string
      Series: TimeSeries list }

type ScalarValue = { Tags: Tag list; Value: float }

type ScalarResult =
    { Formula: string
      Values: ScalarValue list }

// --- running --------------------------------------------------------------------------

let private tags (groupTags: string list) =
    groupTags
    |> List.map (fun t ->
        match t.IndexOf ':' with
        | -1 -> { Key = t; Value = None }
        | i ->
            let value = t[i + 1 ..]
            { Key = t[.. i - 1]; Value = if value = Series.notApplicable then None else Some value })

let private time (ms: int64) = DateTimeOffset.FromUnixTimeMilliseconds ms

/// The names results are labelled with: the formulas, or else the queries.
let private labels (queries: string list) (formulas: string list) =
    if formulas.IsEmpty then queries else formulas

/// The engine's own results, as records.
let private results (names: string list) (outputs: (int * (Series.Series * Series.SeriesExtra) list) list) : FormulaResult list =
    [ for (index, series) in outputs ->
          { Formula = names[index]
            Series =
              [ for (s, extra) in series ->
                    { Tags = tags s.GroupTags
                      Points =
                        [ for p in s.Points ->
                              { Time = time p.Key
                                Value = p.Value
                                Expected =
                                  extra.Band
                                  |> Option.bind (Map.tryFind p.Key)
                                  |> Option.map (fun (lo, hi) -> { Lower = lo; Upper = hi }) } ]
                      Forecast =
                        [ for (t, v, lo, hi) in defaultArg extra.Forecast [] ->
                              { Time = time t
                                Value = v
                                Band = { Lower = lo; Upper = hi } } ] } ] } ]

/// The primitive under the others: an already parsed request — trees, not
/// strings. For code that parses once and runs many times, as a monitor
/// evaluating the same query every minute does.
///
/// `labels` names the results, one per formula (or per query when there are
/// no formulas).
let parsed
    (execute: Sql -> Task<Compile.Row list>)
    (tenant: TenantId)
    (labels: string list)
    (request: Timeseries.ParsedTimeseriesRequest)
    : Task<Result<FormulaResult list, string list>> =
    task {
        match Plan.plan request with
        | Error problems -> return Error problems
        | Ok plan ->
            let! outputs = Execute.evaluate execute tenant plan
            return Ok(results labels outputs)
    }

/// Timeseries: every formula's series over the window.
///
/// Problems with the request — a query that does not parse, a function given
/// the wrong arguments, a formula naming no query — come back as Error, each
/// one named, exactly as the HTTP endpoint reports them.
let timeseries
    (execute: Sql -> Task<Compile.Row list>)
    (tenant: TenantId)
    (request: TimeseriesRequest)
    : Task<Result<FormulaResult list, string list>> =
    task {
        let wire: Wire.TimeseriesFormulaRequestAttributes =
            { From = Some(request.From.ToUnixTimeMilliseconds())
              To = Some(request.To.ToUnixTimeMilliseconds())
              Interval = request.Interval |> Option.map (fun i -> int64 i.TotalMilliseconds)
              Queries = request.Queries |> List.map (fun q -> { DataSource = "metrics"; Name = Some q.Name; Query = q.Query })
              Formulas =
                match request.Formulas with
                | [] -> None
                | fs -> Some(fs |> List.map (fun f -> { Formula = f; Limit = None })) }

        match Timeseries.parse wire with
        | Error problems -> return Error problems
        | Ok request' -> return! parsed execute tenant (labels (request.Queries |> List.map _.Name) request.Formulas) request'
    }

/// Splits an expression with queries inline into named queries and the
/// formula over their names: each metric query becomes q1, q2, …, the same
/// query twice being one.
let split (expr: Expr<MetricQuery>) : Timeseries.ParsedQuery list * Formula =
    let queries = ResizeArray<MetricQuery>()

    let name (q: MetricQuery) =
        match queries |> Seq.tryFindIndex ((=) q) with
        | Some i -> $"q{i + 1}"
        | None ->
            queries.Add q
            $"q{queries.Count}"

    let rec formula (e: Expr<MetricQuery>) : Formula =
        match e with
        | Leaf q -> Leaf(name q)
        | Number n -> Number n
        | Neg inner -> Neg(formula inner)
        | Binary(op, l, r) -> Binary(op, formula l, formula r)
        | Call(fn, args) ->
            Call(
                fn,
                args
                |> List.map (function
                    | Value v -> Value(formula v)
                    | Lit l -> Lit l
                    | Named(n, v) -> Named(n, v))
            )

    let f = formula expr
    let named = queries |> Seq.mapi (fun i q -> ({ Name = Some $"q{i + 1}"; Query = q }: Timeseries.ParsedQuery)) |> List.ofSeq
    named, f

/// One expression with its queries inline, the way Datadog's v1 API and its
/// monitors write them:
///
///   anomalies(avg:system.cpu.user{*} by {host}, 'basic', 3)
///   sum:requests.errors{*}.as_count() / sum:requests.hits{*}.as_count() * 100
///
/// `split` turns it into named queries and a formula; from there it is the
/// same engine as `timeseries`.
let expression
    (execute: Sql -> Task<Compile.Row list>)
    (tenant: TenantId)
    (from: DateTimeOffset)
    (until: DateTimeOffset)
    (text: string)
    : Task<Result<FormulaResult list, string list>> =
    match Parser.parseProgram text with
    | Error e -> Task.FromResult(Error [ e ])
    | Ok [ expr ] ->
        let queries, formula = split expr

        parsed execute tenant [ text ]
            { From = from
              To = until
              Interval = None
              Queries = queries
              Formulas = [ { Formula = formula; Limit = None } ] }
    | Ok many -> Task.FromResult(Error [ $"expected one expression, got {many.Length} separated by commas" ])

/// Scalar: every formula's one value per group, over the window.
let scalar
    (execute: Sql -> Task<Compile.Row list>)
    (tenant: TenantId)
    (request: ScalarRequest)
    : Task<Result<ScalarResult list, string list>> =
    task {
        let wire: Wire.TimeseriesFormulaRequestAttributes =
            { From = Some(request.From.ToUnixTimeMilliseconds())
              To = Some(request.To.ToUnixTimeMilliseconds())
              Interval = None
              Queries = request.Queries |> List.map (fun q -> { DataSource = "metrics"; Name = Some q.Name; Query = q.Query })
              Formulas =
                match request.Formulas with
                | [] -> None
                | fs -> Some(fs |> List.map (fun f -> { Formula = f; Limit = None })) }

        match Timeseries.parse wire |> Result.bind Plan.plan with
        | Error problems -> return Error problems
        | Ok plan ->
            match! Execute.evaluateScalar execute tenant plan (request.Queries |> List.map _.Aggregator) with
            | Error problems -> return Error problems
            | Ok outputs ->
                let names = labels (request.Queries |> List.map _.Name) request.Formulas

                return
                    Ok
                        [ for (name, series) in List.zip names outputs ->
                              { Formula = name
                                Values =
                                  [ for s in series do
                                        // A group whose window held no points has no value.
                                        match s.Points |> Seq.tryHead with
                                        | Some p -> { Tags = tags s.GroupTags; Value = p.Value }
                                        | None -> () ] } ]
    }
