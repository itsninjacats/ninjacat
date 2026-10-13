/// The panel's token on the query port: an HS256 JWT the panel signs for each
/// call (frontend/src/lib/server/ninjacat.ts) and ASP.NET's JwtBearer checks.
/// It names the tenant every query is for; nothing else does.
module NinjaCat.Api.Server.QueryToken

open System
open System.Security.Claims
open System.Text
open System.Threading.Tasks
open Microsoft.AspNetCore.Authentication
open Microsoft.AspNetCore.Authentication.JwtBearer
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Configuration
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.DependencyInjection.Extensions
open Microsoft.IdentityModel.Tokens
open NinjaCat.Api.Engine

let issuer = "ninjacat-panel"
let audience = "ninjacat-query"
let tenantClaim = "tenant"

/// The one tenant a token names. A JSON array in the claim arrives as several
/// claims, and a token for more than one tenant is not a token for the first.
let private namedTenant (principal: ClaimsPrincipal) : string option =
    match principal.FindAll tenantClaim |> Seq.map _.Value |> Seq.toList with
    | [ id ] when id <> "" -> Some id
    | _ -> None

/// The tenant the request's token names. A token without one is refused
/// before any handler runs, so only a route left open (a probe) can get here
/// without it, and that is a wiring mistake rather than a request to answer.
let tenant (ctx: HttpContext) : TenantId =
    match namedTenant ctx.User with
    | Some id -> TenantId id
    | None -> invalidOp "no tenant claim: this route must require the panel's token"

let private requireTenant (context: TokenValidatedContext) : Task =
    if (namedTenant context.Principal).IsNone then
        context.Fail "the token must name exactly one tenant"

    Task.CompletedTask

/// JwtBearer as the default scheme, and authorization to ask for it.
let addTo (services: IServiceCollection) (secret: string) : unit =
    let key = Encoding.UTF8.GetBytes secret

    if key.Length < 32 then
        failwith "NINJACAT_QUERY_TOKEN_SECRET must be at least 32 bytes (openssl rand -hex 32)"

    // AddAuthenticationCore, as in the intake (Intake/Auth.fs): AddAuthentication
    // also brings data protection, whose hosted service writes a key ring to
    // $HOME at startup, and nothing here is encrypted. AddJwtBearer needs one
    // more of its services, the configuration scheme options are read from;
    // every option is set in code below, so that configuration is empty.
    services.AddAuthenticationCore(fun options -> options.DefaultScheme <- JwtBearerDefaults.AuthenticationScheme).AddWebEncoders()
    |> ignore

    services.TryAddSingleton<TimeProvider> TimeProvider.System

    services.TryAddSingleton<IAuthenticationConfigurationProvider>(
        { new IAuthenticationConfigurationProvider with
            member _.AuthenticationConfiguration = ConfigurationBuilder().Build() }
    )

    AuthenticationBuilder(services)
        .AddJwtBearer(fun options ->
            // The claims keep the names the panel gave them (`sub`, `role`),
            // not the URIs .NET would map them to.
            options.MapInboundClaims <- false

            options.TokenValidationParameters <-
                TokenValidationParameters(
                    ValidIssuer = issuer,
                    ValidAudience = audience,
                    IssuerSigningKey = SymmetricSecurityKey key,
                    ValidAlgorithms = [ SecurityAlgorithms.HmacSha256 ],
                    // A token lives 60 seconds; the default five minutes of
                    // skew would make that six.
                    ClockSkew = TimeSpan.FromSeconds 5.0
                )

            options.Events <- JwtBearerEvents(OnTokenValidated = Func<TokenValidatedContext, Task> requireTenant))
    |> ignore

    services.AddAuthorization() |> ignore
