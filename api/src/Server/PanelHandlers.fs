/// The panel's internal API (`/internal/*`): what the SvelteKit server asks
/// for on behalf of a signed-in user. Internal port only; nothing here asks
/// for a key. HTTP in, a query from Engine.Panel, JSON out.
module NinjaCat.Api.Server.PanelHandlers

open System
open System.Collections
open System.Threading.Tasks
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging
open Oxpecker
open ClickHouse.Driver
open NinjaCat.Api.Engine

let private fail (status: int) (message: string) : EndpointHandler =
    setStatusCode status >=> json {| error = message |}

let private queryValue (ctx: HttpContext) (name: string) : string =
    ctx.TryGetQueryValue name |> Option.defaultValue ""

let private queryValues (ctx: HttpContext) (name: string) : string list =
    ctx.Request.Query[name].ToArray() |> List.ofArray

let private queryInt (ctx: HttpContext) (name: string) : int =
    match Int32.TryParse(queryValue ctx name) with
    | true, n -> n
    | false, _ -> 0

/// Until requests carry a tenant of their own, the panel may name one.
let private tenant (ctx: HttpContext) : TenantId =
    match queryValue ctx "tenant" with
    | "" -> TenantId "default"
    | named -> TenantId named

/// Runs a query; a failure in ClickHouse is the caller's 400 with its reason,
/// as the query is built from the caller's parameters.
let private run (ctx: HttpContext) (read: Data.Common.DbDataReader -> 'T) (sql: Sql) : Task<Result<'T list, string>> =
    task {
        try
            let! rows = ClickHouse.query (ctx.GetService<ClickHouseClient>()) ctx.RequestAborted read sql
            return Ok rows
        with e ->
            ctx.GetService<ILoggerFactory>().CreateLogger("NinjaCat.Api.Panel").LogWarning("query: {Error}", e.Message)
            return Error e.Message
    }

/// One-column lookups all answer the same way: `{"<name>": [...]}`.
let private names (sql: Sql) (respond: string list -> obj) : EndpointHandler =
    fun ctx ->
        task {
            match! run ctx _.GetString(0) sql with
            | Ok found -> return! ctx.WriteJson(respond found)
            | Error e -> return! fail 400 e ctx
        }

let metricNames: EndpointHandler =
    fun ctx -> names (Panel.listMetrics (tenant ctx) (queryValue ctx "search") (queryInt ctx "limit")) (fun found -> {| metrics = found |}) ctx

let metricTagKeys: EndpointHandler =
    fun ctx -> names (Panel.listTagKeys (tenant ctx) (queryValue ctx "metric")) (fun found -> {| keys = found |}) ctx

let metricTagValues: EndpointHandler =
    fun ctx ->
        match queryValue ctx "key" with
        | "" -> fail 400 "key is required" ctx
        | key ->
            names
                (Panel.listTagValues (tenant ctx) (queryValue ctx "metric") key (queryValue ctx "search") (queryInt ctx "limit"))
                (fun found -> {| values = found |})
                ctx

/// `from` and `to` of a request: an hour back to now unless given.
let private timeRange (ctx: HttpContext) : Result<DateTimeOffset * DateTimeOffset, string> =
    let now = DateTimeOffset.UtcNow

    match Panel.parseTime (queryValue ctx "from") (now.AddHours -1.0) now, Panel.parseTime (queryValue ctx "to") now now with
    | Error e, _ -> Error("bad from: " + e)
    | _, Error e -> Error("bad to: " + e)
    | Ok from, Ok until when until <= from -> Error "to must be after from"
    | Ok from, Ok until -> Ok(from, until)

/// One metric over a time range, one series per host or per combination of
/// the `by` keys. The query engine runs it, as it runs every other query.
let metricQuery: EndpointHandler =
    fun ctx ->
        task {
            let metric = queryValue ctx "metric"

            match metric, timeRange ctx, Panel.parseTagFilters (queryValues ctx "tag") with
            | "", _, _ -> return! fail 400 "metric is required" ctx
            | _, Error e, _
            | _, _, Error e -> return! fail 400 e ctx
            | _, Ok(from, until), Ok tags ->
                let query: Panel.SeriesQuery =
                    { Metric = metric
                      Hosts = queryValues ctx "host"
                      From = from
                      To = until
                      Step =
                        match Duration.parse (queryValue ctx "step") with
                        | Ok asked when asked > TimeSpan.Zero -> Some asked
                        | _ -> None
                      Aggregation = queryValue ctx "agg"
                      Tags = tags
                      GroupBy = queryValues ctx "by" }

                let execute = ClickHouse.metricRows (ctx.GetService<ClickHouseClient>()) ctx.RequestAborted

                // A failure in ClickHouse is the caller's 400 with its
                // reason, as for the other lookups.
                let! answer =
                    task {
                        try
                            return! Query.Engine.parsed execute (tenant ctx) [ "q" ] (Panel.seriesRequest query)
                        with e ->
                            ctx.GetService<ILoggerFactory>().CreateLogger("NinjaCat.Api.Panel").LogWarning("query: {Error}", e.Message)
                            return Error [ e.Message ]
                    }

                match answer with
                | Error problems -> return! fail 400 (String.concat "; " problems) ctx
                | Ok results ->
                    return!
                        ctx.WriteJson
                            {| metric = metric
                               from = from.UtcDateTime
                               ``to`` = until.UtcDateTime
                               step = int (Query.Plan.step (until - from) query.Step).TotalSeconds
                               series =
                                [ for series in Panel.seriesOf query results ->
                                      {| metric = metric
                                         host = series.Host
                                         name = series.Name
                                         tags = series.Tags
                                         points = [ for time, value in series.Points -> {| t = time.UtcDateTime; v = value |} ] |} ] |}
        }

/// A `Map(String, Array(String))` column as the driver hands it over.
let private tagMap (value: obj) : Map<string, string list> =
    match value with
    | :? IDictionary as map ->
        [ for key in map.Keys -> string key, map[key] :?> IEnumerable |> Seq.cast<obj> |> Seq.map string |> List.ofSeq ]
        |> Map.ofList
    | _ -> Map.empty

let logSearch: EndpointHandler =
    fun ctx ->
        task {
            match timeRange ctx, Panel.parseTagFilters (queryValues ctx "tag") with
            | Error e, _
            | _, Error e -> return! fail 400 e ctx
            | Ok(from, until), Ok tags ->
                let search: Panel.LogSearch =
                    { Tenant = tenant ctx
                      Text = queryValue ctx "q"
                      Service = queryValue ctx "service"
                      Host = queryValue ctx "host"
                      Status = queryValue ctx "status"
                      Tags = tags
                      From = from
                      To = until
                      Limit = queryInt ctx "limit" }

                let rowsSql, countSql = Panel.searchLogs search

                let read (r: Data.Common.DbDataReader) =
                    {| timestamp = DateTimeOffset.FromUnixTimeMilliseconds(r.GetInt64 0).UtcDateTime
                       host = r.GetString 1
                       service = r.GetString 2
                       source = r.GetString 3
                       status = r.GetString 4
                       message = r.GetString 5
                       tags = tagMap (r.GetValue 6) |}

                let! logs = run ctx read rowsSql
                let! count = run ctx (fun r -> Convert.ToUInt64(r.GetValue 0)) countSql

                match logs, count with
                | Ok logs, Ok [ count ] -> return! ctx.WriteJson {| logs = logs; count = count |}
                | Error e, _
                | _, Error e -> return! fail 400 e ctx
                | _ -> return! fail 500 "unexpected answer to the count query" ctx
        }

let logFacets: EndpointHandler =
    fun ctx ->
        task {
            match timeRange ctx with
            | Error e -> return! fail 400 e ctx
            | Ok(from, until) ->
                let facet (which: Panel.LogFacet) =
                    run ctx (fun r -> {| value = r.GetString 0; count = Convert.ToUInt64(r.GetValue 1) |}) (Panel.logFacet (tenant ctx) from until which)

                let! services = facet Panel.ByService
                let! hosts = facet Panel.ByHost
                let! statuses = facet Panel.ByStatus

                match services, hosts, statuses with
                | Ok services, Ok hosts, Ok statuses -> return! ctx.WriteJson {| services = services; hosts = hosts; statuses = statuses |}
                | Error e, _, _
                | _, Error e, _
                | _, _, Error e -> return! fail 400 e ctx
        }

let endpoints =
    [ subRoute
          "/internal"
          [ GET
                [ route "/metrics/names" metricNames
                  route "/metrics/tags" metricTagKeys
                  route "/metrics/tag-values" metricTagValues
                  route "/metrics/query" metricQuery
                  route "/logs/search" logSearch
                  route "/logs/facets" logFacets ] ] ]
