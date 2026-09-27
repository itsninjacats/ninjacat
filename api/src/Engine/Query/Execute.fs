/// Runs a plan: every query it needs, in parallel, then the response.
///
/// The engine never sees ClickHouse. It is handed `execute`, a function from
/// SQL to rows; the server passes one backed by the driver, tests pass one
/// backed by a list.
module NinjaCat.Api.Engine.Query.Execute

open System.Threading.Tasks
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Query.Plan
open NinjaCat.Api.Engine.Query.Compile

let run (execute: Sql -> Task<Row list>) (tenant: TenantId) (plan: Plan) : Task<Api.V2.Wire.TimeseriesFormulaQueryResponse> =
    task {
        // Only the queries some output uses; one named but never shown is not
        // worth a trip to ClickHouse. Each runs once, however many outputs use it.
        // Seasonal extras read their node again, once per past season.
        let seasonalLags (o: Output) =
            match o.Extra with
            | Anomalies((Robust lags | Agile lags), _) -> lags
            | Forecast(_, SeasonalForecast lags, _, _) -> lags
            | _ -> []

        let fetches =
            plan.Outputs
            |> List.collect (fun o -> Evaluate.fetches o.Node @ (seasonalLags o |> List.collect (fun lag -> Evaluate.fetches (Shift(lag, o.Node)))))
            |> List.distinct

        let! fetched =
            fetches
            |> List.map (fun (i, offset) ->
                task {
                    // A shifted read is the same query over a shifted window;
                    // its times are moved back onto the present one.
                    let query = plan.Queries[i]
                    let window = { plan with From = plan.From.AddMilliseconds(float offset); To = plan.To.AddMilliseconds(float offset) }
                    let! rows = execute (compile tenant window query)

                    let keepFrom = keepFromMs window query

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
                            let times = Series.grid (int64 query.Step.TotalMilliseconds) keepFrom (window.To.ToUnixTimeMilliseconds())
                            combined |> List.map (Series.zeroFill times)
                        | Plain _ -> combined

                    let series =
                        aligned
                        |> List.map (fun s -> { s with Points = s.Points |> Seq.map (fun p -> p.Key - offset, p.Value) |> Map.ofSeq })

                    return (i, offset), series
                })
            |> Task.WhenAll

        let sources = fetches |> List.map fst |> List.distinct

        let fetched: Evaluate.Fetched =
            { Series = Map.ofArray fetched
              StepMs = sources |> List.map (fun i -> i, int64 plan.Queries[i].Step.TotalMilliseconds) |> Map.ofList
              ToMs = plan.To.ToUnixTimeMilliseconds() }

        let fromMs = plan.From.ToUnixTimeMilliseconds()
        let toMs = plan.To.ToUnixTimeMilliseconds()
        let windowMs = toMs - fromMs

        // An output's series, with what its outermost function adds beside
        // them. The extras read history before the window, so the node is
        // evaluated from there, and the series trimmed back to the window.
        let evalOutput (o: Output) : (Series.Series * Series.SeriesExtra) list =
            let stepMs = fetched.StepMs[Evaluate.fetches o.Node |> List.head |> fst]
            let visible (s: Series.Series) = { s with Points = s.Points |> Map.filter (fun t _ -> t >= fromMs) }

            let withExtras (historyFrom: int64) (extra: Series.Series -> Series.SeriesExtra) =
                let full = Evaluate.evalFrom fetched historyFrom o.Node
                let kept = full |> List.map visible |> Series.limit o.Limit |> List.map _.GroupTags |> set
                full |> List.filter (fun s -> kept.Contains s.GroupTags) |> List.map (fun s -> visible s, extra s)

            match o.Extra with
            | NoExtra -> Evaluate.evalFrom fetched fromMs o.Node |> Series.limit o.Limit |> List.map (fun s -> s, Series.noExtra)
            | Anomalies(algorithm, bounds) ->
                let historyFrom = fromMs - int64 anomalyWindow * stepMs

                // Each past season's series, by group, on the present times.
                let seasons =
                    seasonalLags o
                    |> List.map (fun lag -> Evaluate.evalFrom fetched historyFrom (Shift(lag, o.Node)) |> List.map (fun s -> s.GroupTags, s.Points) |> Map.ofList)

                withExtras historyFrom (fun s ->
                    let pts = s.Points |> Map.toArray
                    let values = Array.map snd pts

                    let band =
                        match algorithm with
                        | Basic -> Algorithms.basicBand anomalyWindow bounds values
                        | Robust _
                        | Agile _ ->
                            let past =
                                pts |> Array.map (fun (t, _) -> seasons |> List.map (fun m -> m.TryFind s.GroupTags |> Option.bind (Map.tryFind t)) |> Array.ofList)

                            Algorithms.seasonalBand (algorithm.IsAgile) anomalyWindow bounds values past

                    { Series.noExtra with
                        Band =
                            Array.zip pts band
                            |> Array.choose (fun ((t, _), b) -> if t >= fromMs then b |> Option.map (fun b -> t, b) else None)
                            |> Map.ofArray
                            |> Some })
            | Forecast(deviations, SeasonalForecast lags, _, horizon) ->
                let horizonMs = horizon |> Option.map (fun h -> int64 h.TotalMilliseconds) |> Option.defaultValue windowMs
                let future = Series.grid stepMs toMs (toMs + horizonMs) |> Array.ofSeq

                // Season k's read is drawn so that a future time t finds its
                // past value at t − horizon (see Plan.forecastArgs).
                let seasons =
                    lags |> List.map (fun lag -> Evaluate.evalFrom fetched fromMs (Shift(lag, o.Node)) |> List.map (fun s -> s.GroupTags, s.Points) |> Map.ofList)

                withExtras fromMs (fun s ->
                    let past =
                        future |> Array.map (fun t -> seasons |> List.map (fun m -> m.TryFind s.GroupTags |> Option.bind (Map.tryFind (t - horizonMs))) |> Array.ofList)

                    match Array.zip future (Algorithms.seasonalForecast deviations past) |> Array.choose (fun (t, p) -> p |> Option.map (fun (v, lo, hi) -> t, v, lo, hi)) with
                    | [||] -> Series.noExtra
                    | points -> { Series.noExtra with Forecast = Some(List.ofArray points) })
            | Forecast(deviations, LinearForecast model, history, horizon) ->
                let historyMs = history |> Option.map (fun h -> int64 h.TotalMilliseconds) |> Option.defaultValue windowMs
                let horizonMs = horizon |> Option.map (fun h -> int64 h.TotalMilliseconds) |> Option.defaultValue windowMs
                let future = Series.grid stepMs toMs (toMs + horizonMs) |> Array.ofSeq

                let modelName =
                    match model with
                    | SimpleModel -> "simple"
                    | ReactiveModel -> "reactive"
                    | DefaultModel -> "default"

                withExtras (min fromMs (toMs - historyMs)) (fun s ->
                    let pts = s.Points |> Map.toArray |> Array.filter (fun (t, _) -> t >= toMs - historyMs)

                    if pts.Length < 2 then
                        Series.noExtra
                    else
                        let seconds t = float (t - toMs) / 1000.0
                        let predicted = Algorithms.linearForecast modelName deviations (pts |> Array.map (fst >> seconds)) (Array.map snd pts) (future |> Array.map seconds)

                        { Series.noExtra with
                            Forecast = Array.map2 (fun t (v, lo, hi) -> t, v, lo, hi) future predicted |> List.ofArray |> Some })

        return
            plan.Outputs
            |> List.map (fun o -> o.QueryIndex, evalOutput o)
            |> Series.responseWith
    }
