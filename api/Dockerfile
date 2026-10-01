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

# 8080 agent intake, 8081 panel and query API, 10516 logs over TCP.
EXPOSE 8080 8081 10516

# `migrate` as the only argument applies the ClickHouse migrations and exits.
ENTRYPOINT ["dotnet", "/app/ninjacat-api.dll"]
