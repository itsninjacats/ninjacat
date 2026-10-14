/// `/internal/monitors/*`: validation, and what the monitor service stored, for
/// the token's tenant. The panel writes definitions to Postgres itself.
module NinjaCat.Api.Server.MonitorHandlers

open System
open System.Data.Common
open Microsoft.AspNetCore.Http
open Oxpecker
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Platform
open NinjaCat.Api.Server.PanelHandlers

/// `POST validate`: the problems of a monitor in Datadog's JSON, none when it
/// would be evaluated, and the options it carries that are not acted on.
let validate: EndpointHandler =
    fun ctx ->
        task {
            use reader = new IO.StreamReader(ctx.Request.Body)
            let! body = reader.ReadToEndAsync(ctx.RequestAborted)
            let retention = ctx.GetService<Config.Config>().MetricsRetention

            match Validate.monitor retention DateTimeOffset.UtcNow body with
            | Ok definition -> return! ctx.WriteJson {| problems = ([]: string list); notActedOn = definition.Options.NotActedOn |}
            | Error problems -> return! ctx.WriteJson {| problems = problems; notActedOn = ([]: string list) |}
        }

let private time (r: DbDataReader) (i: int) = DateTimeOffset.FromUnixTimeMilliseconds(r.GetInt64 i)

let private optionalTime (r: DbDataReader) (i: int) = if r.IsDBNull i then None else Some(time r i)

// A reduction such as sum can overflow to infinity, which JSON cannot carry.
let private optionalFloat (r: DbDataReader) (i: int) =
    if r.IsDBNull i then None else Some(r.GetDouble i) |> Option.filter Double.IsFinite

let private tags (r: DbDataReader) (i: int) = (r.GetValue i :?> string[]) |> List.ofArray

/// A query built from checked parameters: a failure is ours, not the caller's.
let private answer (read: DbDataReader -> 'T) (respond: 'T list -> obj) (sql: Sql) : EndpointHandler =
    fun ctx ->
        task {
            match! run ctx read sql with
            | Ok rows -> return! ctx.WriteJson(respond rows)
            | Error e -> return! fail 500 e ctx
        }

/// `GET status?ids=1,2`: per monitor, its state, its open groups by state and
/// when it last triggered.
let status: EndpointHandler =
    fun ctx ->
        match Read.parseIds (queryValues ctx "ids") with
        | Error e -> fail 400 e ctx
        | Ok ids ->
            let read (r: DbDataReader) : Read.StatusRow =
                { Monitor = Convert.ToInt64(r.GetValue 0)
                  State = r.GetString 1
                  Open = Convert.ToBoolean(r.GetValue 2)
                  Groups = Convert.ToInt64(r.GetValue 3)
                  LastTriggeredAt = optionalTime r 4 }

            answer read (fun rows -> {| monitors = Read.statuses ids rows |}) (Read.status (QueryToken.tenant ctx) ids) ctx

/// `GET {id}/groups`: the open groups, with their state and latest value.
let groups (monitor: int64) : EndpointHandler =
    fun ctx ->
        let read (r: DbDataReader) : Read.GroupStatus =
            let state = r.GetString 2

            { Group = r.GetString 0
              Tags = tags r 1
              State = state
              Since = time r 3
              Reason = r.GetString 4
              FirstTriggeredAt = (if state = State.OK.Text then None else optionalTime r 5)
              LastTriggeredAt = optionalTime r 6
              Value = optionalFloat r 7
              EvaluatedAt = optionalTime r 8 }

        answer read (fun rows -> {| groups = rows |}) (Read.groups (QueryToken.tenant ctx) monitor) ctx

/// The epoch stands for "no window" in a transition outside a tick.
let private window (r: DbDataReader) (i: int) = optionalTime r i |> Option.filter (fun t -> t.ToUnixTimeMilliseconds() <> 0L)

let private transition (r: DbDataReader) : Read.TransitionRecord =
    { Group = r.GetString 0
      Tags = tags r 1
      At = time r 2
      FromState = State.Status.ofText (r.GetString 3) |> Option.map _.Text
      ToState = r.GetString 4
      Reason = r.GetString 5
      Actor = (if r.IsDBNull 6 then None else Some(r.GetString 6))
      Value = optionalFloat r 7
      Crossed = Some(r.GetString 8) |> Option.filter (fun s -> s <> "none")
      Comparator = r.GetString 9
      Critical = optionalFloat r 10
      Warning = optionalFloat r 11
      CriticalRecovery = optionalFloat r 12
      WarningRecovery = optionalFloat r 13
      WindowFrom = window r 14
      WindowTo = window r 15
      FirstTriggeredAt = optionalTime r 16
      LastTriggeredAt = optionalTime r 17
      MonitorVersion = Convert.ToInt64(r.GetValue 18) }

/// `GET {id}/transitions?from&to[&group]`: the state timeline, with each open
/// group's latest transition before `from` as where it starts.
let transitions (monitor: int64) : EndpointHandler =
    fun ctx ->
        match timeRange ctx with
        | Error e -> fail 400 e ctx
        | Ok(from, until) ->
            task {
                let tenant = QueryToken.tenant ctx
                let group = queryValue ctx "group"
                let! inRange = run ctx transition (Read.transitions tenant monitor from until group)
                let! before = run ctx transition (Read.initial tenant monitor from group)

                match inRange, before with
                | Ok rows, Ok initial ->
                    let rows, truncated = Read.newest rows
                    return! ctx.WriteJson {| initial = initial; transitions = rows; truncated = truncated |}
                | Error e, _
                | _, Error e -> return! fail 500 e ctx
            }

let private evaluation (r: DbDataReader) : Read.EvaluationRecord =
    { Group = r.GetString 0
      At = time r 1
      Value = optionalFloat r 2
      State = r.GetString 3
      Outcome = r.GetString 4
      Detail = Some(r.GetString 5) |> Option.filter (fun s -> s <> "") }

/// `GET {id}/evaluations?from&to[&group]`: the evaluated values, for the graph.
let evaluations (monitor: int64) : EndpointHandler =
    fun ctx ->
        match timeRange ctx with
        | Error e -> fail 400 e ctx
        | Ok(from, until) ->
            let sql = Read.evaluations (QueryToken.tenant ctx) monitor from until (queryValue ctx "group")
            let respond rows : obj =
                let rows, truncated = Read.newest rows
                {| evaluations = rows; truncated = truncated |}

            answer evaluation respond sql ctx

let endpoints =
    [ subRoute
          "/internal/monitors"
          [ POST [ route "/validate" validate ]
            GET
                [ route "/status" status
                  routef "/{%d}/groups" groups
                  routef "/{%d}/transitions" transitions
                  routef "/{%d}/evaluations" evaluations ] ] ]
