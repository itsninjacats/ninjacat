namespace NinjaCat.Api.Intake

open System
open System.IO
open System.Threading.Tasks
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging
open NinjaCat.Api.Storage

/// Where a request may carry its API key.
type KeySource =
    | KeyFromHeader of name: string
    /// `Authorization: Bearer <key>`.
    | KeyFromBearer
    | KeyFromQuery of name: string

/// Decides whether a request may reach the routes: the key it was admitted
/// with (None on a host that needs none), or the answer that refuses it.
type Auth = HttpContext -> ApiKeys.Store -> Result<ApiKeys.Key option, Response>

module Auth =
    let private candidate (http: HttpContext) (source: KeySource) : string =
        let first (values: Microsoft.Extensions.Primitives.StringValues) = if values.Count > 0 then values[0] else ""

        match source with
        | KeyFromHeader name -> first http.Request.Headers[name]
        | KeyFromQuery name -> Query.first http name
        | KeyFromBearer ->
            // Anything that is not a Bearer credential yields nothing, so the
            // next source gets its turn.
            let value = first http.Request.Headers.Authorization

            if value.Length >= 7 && value.Substring(0, 7).Equals("Bearer ", StringComparison.OrdinalIgnoreCase) then
                value.Substring(7).Trim()
            else
                ""

    /// The first source that offers a key decides. 403 is what the agent
    /// understands as "bad key, stop sending".
    let fromSources (sources: KeySource list) : Auth =
        fun http store ->
            let key =
                sources
                |> List.tryPick (fun source ->
                    match candidate http source with
                    | "" -> None
                    | found -> Some found)
                |> Option.defaultValue ""

            match store.Lookup key with
            | Some known -> Ok(Some known)
            | None -> Error(Response.errors 403 [ "invalid API key" ])

    /// The Dd-Api-Key header, then the api_key query parameter.
    ///
    /// The query parameter is there for GET /api/v1/validate, which the agent
    /// calls with the key in the URL; refusing it makes the agent mark itself
    /// unhealthy. A key in a URL leaks into access logs, so no other query
    /// parameter is accepted unless an intake's clients leave no choice.
    let standard: Auth = fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromQuery "api_key" ]

    /// For install.<site>: its clients are anonymous.
    let none: Auth = fun _ _ -> Ok None

/// What every handler needs from the process.
type Deps =
    { Store: ApiKeys.Store
      Sink: ISink
      Log: ILogger
      /// Answer 202 instead of 404 on unknown paths (NINJACAT_ACK_UNKNOWN):
      /// keeps a chatty agent quiet while a new endpoint is looked at.
      AckUnknown: bool }

/// One intake: the routes of one or more Datadog products, behind one guard.
type Engine = { Auth: Auth; Routes: Route list }

module Engine =
    /// The agent sweeps these at startup on whatever host it talks to, before
    /// it knows which intake that is — so every engine answers them.
    let private sharedRoutes: Route list =
        [ Route.get "/api/v1/validate" (fun _ -> Response.json 200 """{"valid":true}""")
          Route.head "/support/flare" Routers.Flare.handle
          Route.post "/support/flare" Routers.Flare.handle
          Route.head "/support/flare/:case_id" Routers.Flare.handle
          Route.post "/support/flare/:case_id" Routers.Flare.handle ]

    let create (auth: Auth) (routes: Route list) : Engine =
        { Auth = auth
          Routes = routes @ sharedRoutes }

    let private segments (path: string) : string list =
        path.Split('/', StringSplitOptions.RemoveEmptyEntries) |> List.ofArray

    /// Matches a path against a pattern. Returns the path parameters and how
    /// specific the match is, segment by segment: 0 literal, 1 `:param`,
    /// 2 `*rest`. Lower wins, so a literal route beats a parameter.
    let private matchPattern (pattern: string) (path: string) : (Map<string, string> * int list) option =
        let rec walk (patternLeft: string list) (pathLeft: string list) (found: Map<string, string>) (rank: int list) =
            match patternLeft, pathLeft with
            | [], [] -> Some(found, List.rev rank)
            | p :: _, rest when p.StartsWith '*' ->
                // Gin gives the rest of the path with its leading slash.
                Some(found.Add(p.Substring 1, "/" + String.Join("/", rest)), List.rev (2 :: rank))
            | p :: patternRest, s :: pathRest when p.StartsWith ':' -> walk patternRest pathRest (found.Add(p.Substring 1, s)) (1 :: rank)
            | p :: patternRest, s :: pathRest when p = s -> walk patternRest pathRest found (0 :: rank)
            | _ -> None

        // "/a/" and "/a" are different routes, as in Gin.
        if pattern.EndsWith '/' <> path.EndsWith '/' && not (pattern.Contains "/*") then
            None
        else
            walk (segments pattern) (segments path) Map.empty []

    let private findRoute (routes: Route list) (method: string) (path: string) : (Route * Map<string, string>) option =
        routes
        |> List.choose (fun route ->
            if route.Method <> method then
                None
            else
                matchPattern route.Pattern path |> Option.map (fun (found, rank) -> rank, (route, found)))
        |> List.sortBy fst
        |> List.tryHead
        |> Option.map snd

    let private unknown (deps: Deps) (http: HttpContext) : Response =
        // Logged loudly on purpose: this is how unhandled endpoints are found.
        deps.Log.LogWarning(
            "UNHANDLED ENDPOINT: {Method} {Path} (Content-Type: {ContentType}, User-Agent: {UserAgent})",
            http.Request.Method,
            http.Request.Path.Value,
            http.Request.ContentType,
            http.Request.Headers.UserAgent.ToString()
        )

        if deps.AckUnknown then Response.json 202 "{}" else Response.errors 404 [ "unknown endpoint" ]

    /// Answers one request: the body is decompressed, the route found, the key
    /// checked, the handler run. Nothing here waits on I/O.
    let respond (deps: Deps) (engine: Engine) (http: HttpContext) (rawBody: byte[]) : Response =
        try
            let method = http.Request.Method
            let path = http.Request.Path.Value

            // The two probes that never need a key.
            if method = "GET" && path = "/ping" then
                Response.json 200 """{"message":"pong"}"""
            elif method = "GET" && path = "/_health" then
                Response.json 200 """{"status":"ok"}"""
            else
                match findRoute engine.Routes method path with
                | None ->
                    // As Gin does: a path that only differs from a route by its
                    // trailing slash is redirected there, 301 for GET and 307
                    // otherwise, so the method and body are kept.
                    let other = if path.EndsWith '/' then path.TrimEnd '/' else path + "/"

                    if path <> "/" && (findRoute engine.Routes method other).IsSome then
                        let location = other + http.Request.QueryString.Value
                        Response.status (if method = "GET" then 301 else 307) |> Response.withHeader "Location" location
                    else
                        unknown deps http
                | Some(route, pathParams) ->
                    match engine.Auth http deps.Store with
                    | Error refusal -> refusal
                    | Ok key ->
                        let encoding = http.Request.Headers.ContentEncoding.ToString()

                        route.Handler
                            { Http = http
                              Body = (if encoding = "" then rawBody else Body.decompress deps.Log encoding rawBody)
                              Params = pathParams
                              Key = key
                              Sink = deps.Sink
                              Log = deps.Log }
        with e ->
            deps.Log.LogError(e, "handler failed: {Method} {Path}", http.Request.Method, http.Request.Path.Value)
            Response.status 500

    let readBody (http: HttpContext) : Task<byte[]> =
        task {
            use buffer = new MemoryStream()
            do! http.Request.Body.CopyToAsync buffer
            return buffer.ToArray()
        }

    let write (http: HttpContext) (response: Response) : Task =
        task {
            http.Response.StatusCode <- response.Status

            for name, value in response.Headers do
                http.Response.Headers[name] <- value

            if response.Body.Length > 0 && http.Request.Method <> "HEAD" then
                http.Response.ContentLength <- int64 response.Body.Length
                do! http.Response.Body.WriteAsync(response.Body, 0, response.Body.Length)
        }
