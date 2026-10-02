# Production image for the server, built in two stages.
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS builder

WORKDIR /src

# Project files first: the restore layer is reused until a dependency changes.
COPY global.json Directory.Build.props Directory.Packages.props NinjaCat.Api.slnx ./
COPY src/Proto/NinjaCat.Api.Proto.csproj src/Proto/
COPY src/Engine/Engine.fsproj src/Engine/
COPY src/Storage/Storage.fsproj src/Storage/
COPY src/Intake/Intake.fsproj src/Intake/
COPY src/Server/Server.fsproj src/Server/
RUN dotnet restore src/Server/Server.fsproj

COPY . .
RUN dotnet publish src/Server/Server.fsproj -c Release --no-restore -o /out

# aspnet, not runtime-deps: the service is framework-dependent, which keeps
# the image layer shared with every other ASP.NET image on the host.
FROM mcr.microsoft.com/dotnet/aspnet:10.0

COPY --from=builder /out /app

# Capture dumps (DEBUG=true) land in the working directory by default.
RUN mkdir /data && chown app:app /data
WORKDIR /data
USER app

# The base image presets port 8080 for ASP.NET. The server names its own
# ports, and would warn at every start that it overrides the preset.
ENV ASPNETCORE_HTTP_PORTS=

# `intake` listens on 8080 (and 10516, logs over TCP), `query` on 8081.
EXPOSE 8080 8081 10516

# The command says which process this container is: `intake`, `query`, or
# `migrate` (apply the ClickHouse migrations and exit). There is no default.
ENTRYPOINT ["/app/ninjacat-api"]
