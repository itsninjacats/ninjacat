namespace NinjaCat.Api.Intake

open System
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
