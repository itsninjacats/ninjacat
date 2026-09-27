/// HTTP in, engine call, JSON out. No SQL and no query logic belongs here.
module NinjaCat.Api.Server.Handlers

open System
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging
open Oxpecker
open ClickHouse.Driver
open NinjaCat.Api.Engine

/// Used until requests carry a tenant — same placeholder as the Go side.
let private defaultTenant = TenantId "default"

/// Datadog's error envelope, so API clients report our errors the way they
/// report Datadog's.
let private errors (status: int) (msgs: string list) : EndpointHandler =
    setStatusCode status >=> json {| errors = msgs |}

let private badRequest (msg: string) : EndpointHandler = errors StatusCodes.Status400BadRequest [ msg ]

let ping: EndpointHandler = text "pong"

let health: EndpointHandler =
    fun ctx ->
        task {
            let! clickhouse = ClickHouse.ping (ctx.GetService<ClickHouseClient>()) ctx.RequestAborted
            let! postgres = Postgres.ping (ctx.GetService<Npgsql.NpgsqlDataSource>()) ctx.RequestAborted
            let status ok = if ok then "ok" else "unreachable"

            if not (clickhouse && postgres) then
                ctx.SetStatusCode StatusCodes.Status503ServiceUnavailable

            return! ctx.WriteJson {| clickhouse = status clickhouse; postgres = status postgres |}
        }

/// `GET /api/v1/metrics?from=<unix seconds>[&host=<name>]`
///
/// `tag_filter` is accepted by Datadog too; it waits for the tag filter parser.
let activeMetrics: EndpointHandler =
    fun ctx ->
        match ctx.TryGetQueryValue "from" |> Option.map Int64.TryParse with
        | Some(true, from) ->
            task {
                let req: Metrics.ActiveMetricsRequest =
                    { Tenant = defaultTenant
                      From = DateTimeOffset.FromUnixTimeSeconds from
                      Host = ctx.TryGetQueryValue "host" |> Option.filter (String.IsNullOrEmpty >> not) }

                let client = ctx.GetService<ClickHouseClient>()
                let! metrics = Metrics.activeMetrics req |> ClickHouse.query client ctx.RequestAborted _.GetString(0)
                return! ctx.WriteJson {| from = string from; metrics = metrics |}
            }
        | Some _ -> badRequest "'from' must be a unix timestamp in seconds" ctx
        | None -> badRequest "'from' is required" ctx

/// `POST /api/v2/query/timeseries`
///
/// Read, plan, run. Everything that can be wrong with the request is a 400
/// listing every problem; a failure while running is a 500 with the reason.
let queryTimeseries: EndpointHandler =
    fun ctx ->
        task {
            use reader = new IO.StreamReader(ctx.Request.Body)
            let! body = reader.ReadToEndAsync(ctx.RequestAborted)

            let planned =
                NinjaCat.Api.Engine.Api.V2.Timeseries.read body
                |> Result.bind NinjaCat.Api.Engine.Query.Plan.plan

            match planned with
            | Error msgs -> return! errors StatusCodes.Status400BadRequest msgs ctx
            | Ok plan ->
                let client = ctx.GetService<ClickHouseClient>()
                let execute = ClickHouse.metricRows client ctx.RequestAborted

                try
                    let! response = NinjaCat.Api.Engine.Query.Execute.run execute defaultTenant plan
                    return! ctx.WriteJson response
                with e ->
                    let log = ctx.GetService<ILoggerFactory>().CreateLogger "NinjaCat.Api.QueryTimeseries"
                    log.LogError(e, "timeseries query failed")
                    return! errors StatusCodes.Status500InternalServerError [ $"query failed: {e.Message}" ] ctx
        }

/// `POST /api/v2/query/scalar`: one value per group and formula, for query
/// value, top list and table widgets.
let queryScalar: EndpointHandler =
    fun ctx ->
        task {
            use reader = new IO.StreamReader(ctx.Request.Body)
            let! body = reader.ReadToEndAsync(ctx.RequestAborted)

            let planned =
                NinjaCat.Api.Engine.Api.V2.Scalar.read body
                |> Result.bind (fun r -> NinjaCat.Api.Engine.Query.Plan.plan r.Request |> Result.map (fun p -> p, r))

            match planned with
            | Error msgs -> return! errors StatusCodes.Status400BadRequest msgs ctx
            | Ok(plan, request) ->
                let execute = ClickHouse.metricRows (ctx.GetService<ClickHouseClient>()) ctx.RequestAborted

                try
                    match! NinjaCat.Api.Engine.Query.Execute.runScalar execute defaultTenant plan request with
                    | Ok response -> return! ctx.WriteJson response
                    | Error msgs -> return! errors StatusCodes.Status400BadRequest msgs ctx
                with e ->
                    let log = ctx.GetService<ILoggerFactory>().CreateLogger "NinjaCat.Api.QueryScalar"
                    log.LogError(e, "scalar query failed")
                    return! errors StatusCodes.Status500InternalServerError [ $"query failed: {e.Message}" ] ctx
        }

let endpoints =
    [ GET [ route "/ping" ping; route "/health" health ]
      subRoute "/api/v1" [ GET [ route "/metrics" activeMetrics ] ]
      subRoute "/api/v2" [ POST [ route "/query/timeseries" queryTimeseries; route "/query/scalar" queryScalar ] ] ]
