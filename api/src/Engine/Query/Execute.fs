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
        let fetches = plan.Outputs |> List.collect (_.Node >> Evaluate.fetches) |> List.distinct

        let! fetched =
            fetches
            |> List.map (fun (i, offset) ->
                task {
                    // A shifted read is the same query over a shifted window;
                    // its times are moved back onto the present one.
                    let query = plan.Queries[i]
                    let window = { plan with From = plan.From.AddMilliseconds(float offset); To = plan.To.AddMilliseconds(float offset) }
                    let! rows = execute (compile tenant window query)

                    let series =
                        Series.fromRows query.GroupBy (keepFromMs window query) rows
                        |> List.map (fun s -> { s with Points = s.Points |> Seq.map (fun p -> p.Key - offset, p.Value) |> Map.ofSeq })

                    return (i, offset), series
                })
            |> Task.WhenAll

        let sources = fetches |> List.map fst |> List.distinct

        let fetched: Evaluate.Fetched =
            { Series = Map.ofArray fetched
              StepMs = sources |> List.map (fun i -> i, int64 plan.Queries[i].Step.TotalMilliseconds) |> Map.ofList }

        let fromMs = plan.From.ToUnixTimeMilliseconds()

        return
            plan.Outputs
            |> List.map (fun o -> o.QueryIndex, Evaluate.evalFrom fetched fromMs o.Node |> Series.limit o.Limit)
            |> Series.response
    }
