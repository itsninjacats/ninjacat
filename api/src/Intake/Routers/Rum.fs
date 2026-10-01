/// Not ported yet: server/intake/router_rum.go.
module NinjaCat.Api.Intake.Routers.Rum

open NinjaCat.Api.Intake

/// The key guard of browser-intake: clients that cannot set a header send
/// the key as `dd-api-key` in the query string.
let gate: Auth = Auth.fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromQuery "dd-api-key" ]

/// What sits above the router on browser-intake: CORS on every answer, and
/// the rewrite of forwarded URLs.
let wrap (inner: Microsoft.AspNetCore.Http.HttpContext -> byte[] -> Response) : Microsoft.AspNetCore.Http.HttpContext -> byte[] -> Response = inner

let routes: Route list = []
