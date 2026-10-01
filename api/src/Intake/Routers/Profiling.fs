/// Not ported yet: server/intake/router_profiling.go.
module NinjaCat.Api.Intake.Routers.Profiling

open NinjaCat.Api.Intake

/// POST of a profile; `label` names the intake in logs and raw_payloads.
let handleProfile (label: string) : Handler = fun _ -> Response.status 501

let handleDebugger: Handler = fun _ -> Response.status 501

let profileRoutes: Route list = []

let debuggerRoutes: Route list = []

let sourcemapRoutes: Route list = []
