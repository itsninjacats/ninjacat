/// The panel's token on the query port: an HS256 JWT the panel signs for each
/// call (frontend/src/lib/server/ninjacat.ts) and ASP.NET's JwtBearer checks.
/// It names the tenant every query is for; nothing else does.
module NinjaCat.Api.Server.QueryToken

open System
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

let tenant (ctx: HttpContext) = TenantId(ctx.User.FindFirst(tenantClaim).Value)

// A JSON array in the claim arrives as several claims; a token for more than
// one tenant is not a token for the first.
let private requireTenant (context: TokenValidatedContext) : Task =
    match context.Principal.FindAll tenantClaim |> Seq.map _.Value |> Seq.toList with
    | [ id ] when id <> "" -> ()
    | _ -> context.Fail "the token must name exactly one tenant"

    Task.CompletedTask

/// JwtBearer as the default scheme, and authorization to ask for it.
let addTo (services: IServiceCollection) (secret: string) : unit =
    let key = Encoding.UTF8.GetBytes secret

    if key.Length < 32 then
        failwith "NINJACAT_QUERY_TOKEN_SECRET must be at least 32 bytes (openssl rand -hex 32)"

    // AddAuthenticationCore as in Intake/Auth.fs (AddAuthentication brings data
    // protection, which writes a key ring to $HOME); JwtBearer then needs the
    // empty configuration provider below.
    services.AddAuthenticationCore(fun options -> options.DefaultScheme <- JwtBearerDefaults.AuthenticationScheme).AddWebEncoders()
    |> ignore

    services.TryAddSingleton<TimeProvider> TimeProvider.System

    services.TryAddSingleton<IAuthenticationConfigurationProvider>(
        { new IAuthenticationConfigurationProvider with
            member _.AuthenticationConfiguration = ConfigurationBuilder().Build() }
    )

    AuthenticationBuilder(services)
        .AddJwtBearer(fun options ->
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
