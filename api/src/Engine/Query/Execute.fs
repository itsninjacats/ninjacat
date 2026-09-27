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
        let sources = plan.Outputs |> List.collect (_.Node >> Evaluate.sources) |> List.distinct

        let! fetched =
            sources
            |> List.map (fun i ->
                task {
                    let query = plan.Queries[i]
                    let! rows = execute (compile tenant plan query)
                    return i, Series.fromRows query.GroupBy (plan.From.ToUnixTimeMilliseconds()) rows
                })
            |> Task.WhenAll

        let bySource = Map.ofArray fetched

        return
            plan.Outputs
            |> List.map (fun o -> o.QueryIndex, Evaluate.eval bySource o.Node |> Series.limit o.Limit)
            |> Series.response
    }
