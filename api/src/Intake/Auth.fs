namespace NinjaCat.Api.Intake

open System
open System.Security.Claims
open System.Text.Encodings.Web
open System.Threading.Tasks
open Microsoft.AspNetCore.Authentication
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.DependencyInjection.Extensions
open Microsoft.Extensions.Logging
open Microsoft.Extensions.Options

/// Where a request may carry its API key.
type KeySource =
    | KeyFromHeader of name: string
    /// `Authorization: Bearer <key>`.
    | KeyFromBearer
    | KeyFromQuery of name: string

/// The options of one authentication scheme: where it looks for the key.
type ApiKeyOptions() =
    inherit AuthenticationSchemeOptions()
    member val Sources: KeySource list = [] with get, set

/// Datadog API keys as an ASP.NET authentication scheme. A request whose key
/// is known is admitted as that key, with its tenant as a claim.
type ApiKeyHandler(options: IOptionsMonitor<ApiKeyOptions>, logger: ILoggerFactory, encoder: UrlEncoder, store: ApiKeys.Store) =
    inherit AuthenticationHandler<ApiKeyOptions>(options, logger, encoder)

    member private this.Candidate(source: KeySource) : string =
        match source with
        | KeyFromHeader name -> Ctx.header this.Context name
        | KeyFromQuery name -> Ctx.query this.Context name
        | KeyFromBearer ->
            // Anything that is not a Bearer credential yields nothing, so the
            // next source gets its turn.
            let value = Ctx.header this.Context "Authorization"

            if value.Length >= 7 && value.Substring(0, 7).Equals("Bearer ", StringComparison.OrdinalIgnoreCase) then
                value.Substring(7).Trim()
            else
                ""

    /// The first source that offers a key decides.
    override this.HandleAuthenticateAsync() : Task<AuthenticateResult> =
        let key =
            this.Options.Sources
            |> List.tryPick (fun source ->
                match this.Candidate source with
                | "" -> None
                | found -> Some found)
            |> Option.defaultValue ""

        match store.Lookup key with
        | Some known ->
            let claims = [ Claim("key_id", known.ID); Claim("key_name", known.Name); Claim("tenant", known.TenantID) ]
            let principal = ClaimsPrincipal(ClaimsIdentity(claims, this.Scheme.Name))
            Task.FromResult(AuthenticateResult.Success(AuthenticationTicket(principal, this.Scheme.Name)))
        | None -> Task.FromResult(AuthenticateResult.Fail "invalid API key")

    /// 403 is what the agent understands as "bad key, stop sending"; ASP.NET's
    /// own answer to a request without credentials would be 401.
    override this.HandleChallengeAsync(_: AuthenticationProperties) : Task =
        this.Response.StatusCode <- 403
        this.Response.WriteAsJsonAsync {| errors = [ "invalid API key" ] |}

/// The schemes of the intake: which of them a host asks for is said where the
/// host is mounted (Routes.fs).
module ApiKeyAuth =
    /// The Dd-Api-Key header, then the api_key query parameter.
    ///
    /// The query parameter is there for GET /api/v1/validate, which the agent
    /// calls with the key in the URL; refusing it makes the agent mark itself
    /// unhealthy. A key in a URL leaks into access logs, so no other query
    /// parameter is accepted unless an intake's clients leave no choice.
    let agent = "agent"

    /// browser-intake: the header (iOS, Android) or the query string
    /// (browser). The browser has no choice: its fetch sets no headers at
    /// all, to stay a "simple request" and interchangeable with sendBeacon.
    /// The one request the browser SDK does put a header on, the profiler's
    /// quota check, names it DD-CLIENT-TOKEN.
    let browser = "browser"

    /// webhook-intake: Jenkins sends the header, but a GitLab project webhook
    /// has no field for one, so the key travels as `?dd-api-key=`.
    let ciWebhook = "ci-webhook"

    /// data-obs-intake: OpenLineage's transport sends `Authorization:
    /// Bearer`, and the trace-agent's lineage proxy forwards it unchanged.
    let lineage = "lineage"

    /// Registers the four schemes, and authorization to ask for them.
    let addTo (services: IServiceCollection) : unit =
        let scheme (name: string) (sources: KeySource list) (builder: AuthenticationBuilder) : AuthenticationBuilder =
            builder.AddScheme<ApiKeyOptions, ApiKeyHandler>(name, (fun options -> options.Sources <- sources))

        // AddAuthenticationCore, not AddAuthentication: the second also brings
        // ASP.NET's data protection, which creates a key ring on disk at
        // startup. Nothing here encrypts anything (no cookies), so only what
        // a scheme needs is registered: the core, the encoder its handler
        // takes, and the clock.
        services.AddAuthenticationCore().AddWebEncoders() |> ignore
        services.TryAddSingleton<TimeProvider> TimeProvider.System

        AuthenticationBuilder(services)
        |> scheme agent [ KeyFromHeader "Dd-Api-Key"; KeyFromQuery "api_key" ]
        |> scheme browser [ KeyFromHeader "Dd-Api-Key"; KeyFromQuery "dd-api-key"; KeyFromHeader "Dd-Client-Token" ]
        |> scheme ciWebhook [ KeyFromHeader "Dd-Api-Key"; KeyFromQuery "dd-api-key"; KeyFromQuery "api_key" ]
        |> scheme lineage [ KeyFromHeader "Dd-Api-Key"; KeyFromBearer; KeyFromQuery "api_key" ]
        |> ignore

        services.AddAuthorization() |> ignore

/// What the intake is told by whoever starts it.
type IntakeSettings =
    { /// Answer 202 instead of 404 on unknown paths (NINJACAT_ACK_UNKNOWN):
      /// keeps a chatty agent quiet while a new endpoint is looked at.
      AckUnknown: bool }
