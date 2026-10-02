/// Runs a plan: every query it needs, in parallel, then the response.
///
/// The engine never sees ClickHouse. It is handed `execute`, a function from
/// SQL to rows; the server passes one backed by the driver, tests pass one
/// backed by a list.
module NinjaCat.Api.Engine.Query.Execute

open System
open System.Threading.Tasks
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Query.Plan
open NinjaCat.Api.Engine.Query.Compile

/// Reads what `nodes` need over `window`, at the steps in `window.Queries`.
///
/// Only the queries some node uses; one named but never shown is not worth a
/// trip to ClickHouse. Each (query, shift) runs once however many nodes use
/// it, and all run in parallel.
let private fetchAll (execute: Sql -> Task<Row list>) (tenant: TenantId) (window: Plan) (nodes: Node list) : Task<Evaluate.Fetched> =
    task {
        let keys = nodes |> List.collect Evaluate.fetches |> List.distinct

        let! series =
            keys
            |> List.map (fun (i, offset) ->
                task {
                    // A shifted read is the same query over a shifted window;
                    // its times are moved back onto the present one.
                    let query = window.Queries[i]
                    let shifted = { window with From = window.From.AddMilliseconds(float offset); To = window.To.AddMilliseconds(float offset) }
                    let! rows = execute (compile tenant shifted query)
                    let keepFrom = keepFromMs shifted query

                    let combined =
                        match query.Fill with
                        | NoFill -> Series.fromRows query.GroupBy keepFrom rows
                        | _ -> Series.combine query.GroupBy query.SpaceAgg query.Fill keepFrom rows

                    // "COUNT or RATE metrics queried as as_count() or
                    // as_rate() ... are always aligned as 0" (docs:
                    // dashboards/functions/interpolation): an empty bucket
                    // there is zero events, not unknown.
                    let aligned =
                        match query.TimeAgg with
                        | AsCount
                        | AsRate ->
                            let times = Series.grid (int64 query.Step.TotalMilliseconds) keepFrom (shifted.To.ToUnixTimeMilliseconds())
                            combined |> List.map (Series.zeroFill times)
                        | Plain _ -> combined

                    return
                        (i, offset),
                        aligned |> List.map (fun s -> { s with Points = s.Points |> Seq.map (fun p -> p.Key - offset, p.Value) |> Map.ofSeq })
                })
            |> Task.WhenAll

        return
            { Evaluate.Fetched.Series = Map.ofArray series
              Evaluate.Fetched.StepMs =
                keys
                |> List.map fst
                |> List.distinct
                |> List.map (fun i -> i, int64 window.Queries[i].Step.TotalMilliseconds)
                |> Map.ofList
              Evaluate.Fetched.ToMs = window.To.ToUnixTimeMilliseconds() }
    }

/// A seasonal model's input: one series' values at the model's step, from
/// `fromMs` to `toMs`, gaps filled by linear interpolation. None when there is
/// too little to fit: fewer than `seasons` full seasons since the first real
/// point, or more than a fifth of the buckets missing.
///
/// WARNING(undocumented): how Datadog treats gaps in the history it fits.
/// Short gaps are interpolated; mostly-empty history gives no model.
let private modelInput (s: Seasonality) (seasons: int) (fromMs: int64) (toMs: int64) (points: Map<int64, float>) : float[] option =
    let step = int64 s.Step.TotalMilliseconds
    let grid = Series.grid step fromMs toMs |> Array.ofSeq
    let known = grid |> Array.map points.TryFind

    match known |> Array.tryFindIndex Option.isSome with
    | None -> None
    | Some first ->
        let span = known[first..]
        let present = span |> Array.filter Option.isSome |> Array.length

        if span.Length < seasons * s.Period || float present < 0.8 * float span.Length then
            None
        else
            let idx = span |> Array.indexed |> Array.choose (fun (i, v) -> v |> Option.map (fun v -> i, v))

            span
            |> Array.mapi (fun i v ->
                match v with
                | Some v -> v
                | None ->
                    // Between the nearest real points; past the last, the last.
                    let before = idx |> Array.filter (fun (j, _) -> j < i) |> Array.tryLast
                    let after = idx |> Array.tryFind (fun (j, _) -> j > i)

                    match before, after with
                    | Some(j0, v0), Some(j1, v1) -> v0 + (v1 - v0) * float (i - j0) / float (j1 - j0)
                    | Some(_, v0), None -> v0
                    | None, Some(_, v1) -> v1
                    | None, None -> 0.0)
            |> Some

/// Runs a timeseries plan: for each output, in plan order, its query_index
/// and its series with their extras.
let evaluate (execute: Sql -> Task<Row list>) (tenant: TenantId) (plan: Plan) : Task<Series.OutputResult list> =
    task {
        let! fetched = fetchAll execute tenant plan (plan.Outputs |> List.map _.Node)

        let fromMs = plan.From.ToUnixTimeMilliseconds()
        let toMs = plan.To.ToUnixTimeMilliseconds()
        let windowMs = toMs - fromMs

        /// The node's series over a seasonal model's history, at its step:
        /// the same queries and functions, a longer window, a coarser step.
        let history (s: Seasonality) (node: Node) =
            task {
                let step = s.Step
                let window =
                    { plan with
                        From = plan.To - seasonalHistory s
                        Step = step
                        Queries = plan.Queries |> List.map (fun q -> { q with Step = step }) }

                let! h = fetchAll execute tenant window [ node ]
                let series = Evaluate.evalFrom h (window.From.ToUnixTimeMilliseconds()) node
                return window.From.ToUnixTimeMilliseconds(), series |> List.map (fun s -> s.GroupTags, s.Points) |> Map.ofList
            }

        // An output's series, with what its outermost function adds beside
        // them. The extras read history before the window, so the node is
        // evaluated from there, and the series trimmed back to the window.
        let evalOutput (o: Output) : Task<Series.OutputSeries list> =
            task {
                let stepMs = fetched.StepMs[Evaluate.fetches o.Node |> List.head |> fst]
                let visible (s: Series.Series) = { s with Points = s.Points |> Map.filter (fun t _ -> t >= fromMs) }

                let withExtras (historyFrom: int64) (extra: Series.Series -> Series.SeriesExtra) =
                    let full = Evaluate.evalFrom fetched historyFrom o.Node
                    let kept = full |> List.map visible |> Series.limit o.Limit |> List.map _.GroupTags |> set
                    full
                    |> List.filter (fun s -> kept.Contains s.GroupTags)
                    |> List.map (fun s -> ({ Data = visible s; Extra = extra s }: Series.OutputSeries))

                /// A band from expected values: `bounds` standard deviations,
                /// the model's spread combined with the spread inside a model
                /// bucket.
                ///
                /// The model's spread comes from its fit on history, so a
                /// lasting anomaly in the window cannot widen the band around
                /// itself ("predictions remain constant even through
                /// long-lasting anomalies"). The spread inside a bucket covers
                /// a query finer than the model — 20 s points around an hourly
                /// expectation — measured as each point's distance from its
                /// own bucket's mean, which a level shift does not change.
                ///
                /// WARNING(undocumented): how Datadog widens a band for a query
                /// finer than its model. σ² = σ²(model) + σ²(within bucket),
                /// the latter over the `anomalyWindow` points before each.
                let bandFrom (bounds: float) (modelSigma: float) (bucket: int64 -> int64) (pts: (int64 * float)[]) (expected: int64 -> float option) =
                    let bucketMean = pts |> Array.groupBy (fst >> bucket) |> Array.map (fun (b, xs) -> b, Array.averageBy snd xs) |> Map.ofArray
                    let within = pts |> Array.map (fun (t, v) -> v - bucketMean[bucket t])

                    pts
                    |> Array.mapi (fun i (t, _) ->
                        expected t
                        |> Option.map (fun e ->
                            let recent = within[max 0 (i - anomalyWindow) .. i - 1]

                            let withinSigma =
                                if recent.Length >= 5 then sqrt (recent |> Array.averageBy (fun d -> d * d)) else 0.0

                            let sigma = sqrt (modelSigma * modelSigma + withinSigma * withinSigma)
                            t, ({ Lower = e - bounds * sigma; Upper = e + bounds * sigma }: Series.Range)))
                    |> Array.choose id
                    |> Array.filter (fun (t, _) -> t >= fromMs)
                    |> Map.ofArray

                match o.Extra with
                | NoExtra ->
                    return
                        Evaluate.evalFrom fetched fromMs o.Node
                        |> Series.limit o.Limit
                        |> List.map (fun s -> ({ Data = s; Extra = Series.noExtra }: Series.OutputSeries))

                | Anomalies(Basic, bounds) ->
                    return
                        withExtras (fromMs - int64 anomalyWindow * stepMs) (fun s ->
                            let pts = s.Points |> Map.toArray
                            let band = Algorithms.basicBand anomalyWindow bounds (Array.map snd pts)

                            { Series.noExtra with
                                Band =
                                    Array.zip pts band
                                    |> Array.choose (fun ((t, _), b) ->
                                        if t >= fromMs then
                                            b |> Option.map (fun (lower, upper) -> t, ({ Lower = lower; Upper = upper }: Series.Range))
                                        else
                                            None)
                                    |> Map.ofArray
                                    |> Some })

                | Anomalies((Robust season | Agile season) as algorithm, bounds) ->
                    let! historyFrom, histories = history season o.Node
                    let modelStep = int64 season.Step.TotalMilliseconds
                    let bucket t = t / modelStep * modelStep

                    return
                        withExtras (fromMs - int64 anomalyWindow * stepMs) (fun s ->
                            let pts = s.Points |> Map.toArray

                            // Expected value per model bucket, and the model's
                            // own spread, from history before the window.
                            let model =
                                histories.TryFind s.GroupTags
                                |> Option.bind (fun h ->
                                    let firstInWindow = bucket fromMs

                                    match algorithm with
                                    | Robust _ ->
                                        // STL on the history before the
                                        // window; the window is expected at
                                        // the last trend level plus the
                                        // season's profile — so nothing inside
                                        // the window moves the expectation.
                                        modelInput season 3 historyFrom firstInWindow h
                                        |> Option.map (fun y ->
                                            let d = Models.stl season.Period y
                                            let start = firstInWindow - int64 y.Length * modelStep
                                            let level = Array.last d.Trend
                                            // The season's latest cycle is its profile.
                                            let lastCycle = d.Seasonal.Length - season.Period
                                            let phase t = int (((t - start) / modelStep) % int64 season.Period)

                                            let sd =
                                                let m = Array.average d.Remainder
                                                sqrt (d.Remainder |> Array.averageBy (fun r -> (r - m) * (r - m)))

                                            (fun t -> Some(level + d.Seasonal[lastCycle + phase (bucket t)])), sd)
                                    | _ ->
                                        // The airline model fitted before the
                                        // window, then one step ahead through
                                        // it: each bucket expected from those
                                        // before it, the window's own included.
                                        modelInput season 3 historyFrom toMs h
                                        |> Option.bind (fun y ->
                                            let start = toMs - int64 y.Length * modelStep |> bucket
                                            let fitLength = int ((firstInWindow - start) / modelStep)

                                            if fitLength < 3 * season.Period then
                                                None
                                            else
                                                let m = Models.fitAirline season.Period y[.. fitLength - 1]
                                                let p = Models.predictions m y
                                                let at t = let i = int ((bucket t - start) / modelStep) in if i >= 0 && i < p.Length then p[i] else None
                                                Some(at, sqrt m.Sigma2)))

                            match model with
                            | None -> Series.noExtra
                            | Some(expected, sigma) -> { Series.noExtra with Band = Some(bandFrom bounds sigma bucket pts expected) })

                | Forecast(deviations, SeasonalForecast season, _, horizon) ->
                    let! historyFrom, histories = history season o.Node
                    let modelStep = int64 season.Step.TotalMilliseconds
                    let horizonMs = horizon |> Option.map (fun h -> int64 h.TotalMilliseconds) |> Option.defaultValue windowMs
                    let future = Series.grid stepMs toMs (toMs + horizonMs) |> Array.ofSeq

                    return
                        withExtras fromMs (fun s ->
                            // "Requires at least two seasons of history."
                            match histories.TryFind s.GroupTags |> Option.bind (modelInput season 2 historyFrom toMs) with
                            | None -> Series.noExtra
                            | Some y ->
                                let lastBucket = (toMs - 1L) / modelStep * modelStep
                                let steps = int ((toMs + horizonMs - lastBucket) / modelStep) + 1
                                let m = Models.fitAirline season.Period y
                                let predicted = Models.forecastAirline m y steps

                                let points =
                                    future
                                    |> Array.choose (fun t ->
                                        let k = int ((t / modelStep * modelStep - lastBucket) / modelStep) - 1

                                        if k >= 0 && k < predicted.Length then
                                            let v, se = predicted[k]
                                            Some(
                                                { TimeMs = t
                                                  Value = v
                                                  Range = { Lower = v - deviations * se; Upper = v + deviations * se } }
                                                : Series.Predicted
                                            )
                                        else
                                            None)

                                if points.Length = 0 then Series.noExtra
                                else { Series.noExtra with Forecast = Some(List.ofArray points) })

                | Forecast(deviations, LinearForecast model, history, horizon) ->
                    let historyMs = history |> Option.map (fun h -> int64 h.TotalMilliseconds) |> Option.defaultValue windowMs
                    let horizonMs = horizon |> Option.map (fun h -> int64 h.TotalMilliseconds) |> Option.defaultValue windowMs
                    let future = Series.grid stepMs toMs (toMs + horizonMs) |> Array.ofSeq

                    let modelName =
                        match model with
                        | SimpleModel -> "simple"
                        | ReactiveModel -> "reactive"
                        | DefaultModel -> "default"

                    return
                        withExtras (min fromMs (toMs - historyMs)) (fun s ->
                            let pts = s.Points |> Map.toArray |> Array.filter (fun (t, _) -> t >= toMs - historyMs)

                            if pts.Length < 2 then
                                Series.noExtra
                            else
                                let seconds t = float (t - toMs) / 1000.0

                                let predicted =
                                    Algorithms.linearForecast modelName deviations (pts |> Array.map (fst >> seconds)) (Array.map snd pts) (future |> Array.map seconds)

                                { Series.noExtra with
                                    Forecast =
                                        Array.map2
                                            (fun t (p: Algorithms.Prediction) ->
                                                ({ TimeMs = t; Value = p.Value; Range = { Lower = p.Lower; Upper = p.Upper } }: Series.Predicted))
                                            future
                                            predicted
                                        |> List.ofArray
                                        |> Some })
            }

        let! outputs =
            plan.Outputs
            |> List.map (fun o ->
                task {
                    let! series = evalOutput o
                    return ({ QueryIndex = o.QueryIndex; Lines = series }: Series.OutputResult)
                })
            |> Task.WhenAll

        return List.ofArray outputs
    }

/// Runs a timeseries plan into Datadog's response shape.
let run (execute: Sql -> Task<Row list>) (tenant: TenantId) (plan: Plan) : Task<Api.V2.Wire.TimeseriesFormulaQueryResponse> =
    task {
        let! outputs = evaluate execute tenant plan
        return Series.responseWith outputs
    }

// --- scalar ---------------------------------------------------------------------------

open NinjaCat.Api.Engine.Api.V2.Scalar
open NinjaCat.Api.Engine.Api.V2.Wire

/// One series' points in the window as one value.
let private reduce (agg: ScalarAggregator) (stepMs: int64) (points: (int64 * float)[]) : float option =
    if points.Length = 0 then
        None
    else
        let values = Array.map snd points

        match agg with
        | ScalarAvg -> Array.average values
        | ScalarMin -> Array.min values
        | ScalarMax -> Array.max values
        | ScalarSum -> Array.sum values
        | ScalarLast -> points |> Array.maxBy fst |> snd
        | ScalarL2norm -> values |> Array.sumBy (fun v -> v * v) |> sqrt
        | ScalarArea -> Array.sum values * float stepMs / 1000.0
        |> Some

/// Runs a scalar request: each query's window reduced to one value by its
/// aggregator, then the formulas over those values.
///
/// The aggregator belongs to the query, not the formula (spec:
/// MetricsScalarQuery.aggregator), so reduction comes first: `a / b` with avg
/// is avg(a) / avg(b). A reduced series is a series with one point, which is
/// all the evaluator needs — every function and all arithmetic work on it
/// unchanged; time-wise ones have one point to work with.
///
/// WARNING(undocumented): what a sum or area of a gauge adds up. The window's
/// buckets as the timeseries endpoint would draw them — the same points a
/// graph of the query shows.
/// Runs a scalar plan: for each output, in plan order, its series, each with
/// one point — the reduced value — at the window's start.
let evaluateScalar
    (execute: Sql -> Task<Row list>)
    (tenant: TenantId)
    (plan: Plan)
    (aggregators: ScalarAggregator list)
    : Task<Result<Series.Series list list, string list>> =
    task {
        match plan.Outputs |> List.filter (fun o -> o.Extra <> NoExtra) with
        | _ :: _ -> return Error [ "anomalies() and forecast() are not available in scalar queries" ]
        | [] ->
            let! fetched = fetchAll execute tenant plan (plan.Outputs |> List.map _.Node)
            let fromMs = plan.From.ToUnixTimeMilliseconds()
            let toMs = plan.To.ToUnixTimeMilliseconds()
            let aggregators = Array.ofList aggregators

            let reduced =
                { fetched with
                    Series =
                        fetched.Series
                        |> Map.map (fun (i, _) series ->
                            let stepMs = fetched.StepMs[i]

                            series
                            |> List.map (fun s ->
                                let window = s.Points |> Map.toArray |> Array.filter (fun (t, _) -> t >= fromMs && t < toMs)

                                { s with
                                    Points =
                                        match reduce aggregators[i] stepMs window with
                                        | Some v -> Map [ fromMs, v ]
                                        | None -> Map.empty })) }

            let outputs =
                plan.Outputs
                |> List.map (fun o -> Evaluate.evalFrom reduced fromMs o.Node |> Series.limit o.Limit)

            return Ok outputs
    }

/// Runs a scalar plan into Datadog's response shape: group columns, then one
/// number column per formula.
let runScalar
    (execute: Sql -> Task<Row list>)
    (tenant: TenantId)
    (plan: Plan)
    (request: ParsedScalarRequest)
    : Task<Result<ScalarFormulaQueryResponse, string list>> =
    task {
        match! evaluateScalar execute tenant plan request.Aggregators with
        | Error es -> return Error es
        | Ok outputs ->
            let fromMs = plan.From.ToUnixTimeMilliseconds()

            // Rows: every group any column has, in the order they first appear
            // — the first formula's, limit and all, then any others'.
            //
            // WARNING(undocumented): a limit on one formula of several. The
            // rows are the groups every limited formula kept: a top 5 with a
            // second, unlimited column is still five rows, not five plus the
            // rest with a null first column.
            let limited =
                List.zip plan.Outputs outputs
                |> List.filter (fun (o, _) -> o.Limit.IsSome)
                |> List.map (fun (_, series) -> series |> List.map (_.GroupTags >> set) |> Set.ofList)

            let rows =
                outputs
                |> List.concat
                |> List.map _.GroupTags
                |> List.distinctBy set
                |> List.filter (fun tags -> limited |> List.forall (fun kept -> kept.Contains(set tags)))

            let split (tag: string) =
                match tag.IndexOf ':' with
                | -1 -> tag, tag
                | i -> tag[.. i - 1], tag[i + 1 ..]

            let keys =
                match rows |> List.tryFind (List.isEmpty >> not) with
                | Some tags -> tags |> List.map (split >> fst)
                | None -> []

            let groupColumns =
                keys
                |> List.map (fun key ->
                    box
                        { GroupScalarColumn.Name = key
                          Type = "group"
                          Values =
                            rows
                            |> List.map (fun tags ->
                                tags |> List.map split |> List.filter (fst >> (=) key) |> List.map snd) })

            let dataColumns =
                List.zip request.ColumnNames outputs
                |> List.map (fun (name, series) ->
                    let byGroup = series |> List.map (fun s -> set s.GroupTags, s) |> Map.ofList

                    box
                        { DataScalarColumn.Name = name
                          Type = "number"
                          Values = rows |> List.map (fun tags -> byGroup.TryFind(set tags) |> Option.bind (fun s -> s.Points.TryFind fromMs))
                          Meta = { Unit = None } })

            return
                Ok
                    { Data =
                        { Type = "scalar_response"
                          Attributes = { Columns = groupColumns @ dataColumns } } }
    }
