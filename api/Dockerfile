# Production images for the server's two programs, from one build context:
#
#   docker build --target api .        ninjacat-api: intake, query, migrate
#   docker build --target monitors .   ninjacat-monitors
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS builder

# The SDK reports its use to Microsoft unless told not to.
ENV DOTNET_CLI_TELEMETRY_OPTOUT=1

WORKDIR /src

# Project files first: the restore layer is reused until a dependency changes.
COPY global.json Directory.Build.props Directory.Packages.props NinjaCat.Api.slnx ./
COPY src/Proto/NinjaCat.Api.Proto.csproj src/Proto/
COPY src/Engine/Engine.fsproj src/Engine/
COPY src/Storage/Storage.fsproj src/Storage/
COPY src/Intake/Intake.fsproj src/Intake/
COPY src/Platform/Platform.fsproj src/Platform/
COPY src/Server/Server.fsproj src/Server/
COPY src/Monitors/Monitors.fsproj src/Monitors/
RUN dotnet restore src/Server/Server.fsproj \
    && dotnet restore src/Monitors/Monitors.fsproj

COPY . .

# VERSION (1.2.3 or v1.2.3) stamps the assemblies: the monitors' Akka cluster
# compares it so a rollout moves shards to newer pods. 0.0.0 is older than any
# release; the SDK default 1.0.0 would not be.
FROM builder AS publish-api
ARG VERSION=0.0.0
RUN dotnet publish src/Server/Server.fsproj -c Release --no-restore -o /out -p:Version=${VERSION#v}

FROM builder AS publish-monitors
ARG VERSION=0.0.0
RUN dotnet publish src/Monitors/Monitors.fsproj -c Release --no-restore -o /out -p:Version=${VERSION#v}

# aspnet, not runtime-deps: both programs are framework-dependent, which keeps
# the image layer shared with every other ASP.NET image on the host.
FROM mcr.microsoft.com/dotnet/aspnet:10.0 AS runtime

# Npgsql asks for GSS encryption before anything else and loads this library
# to do it. Without it every start prints "Cannot load library
# libgssapi_krb5.so.2", and Kerberos authentication to Postgres cannot work.
RUN apt-get update \
    && apt-get install -y --no-install-recommends libgssapi-krb5-2 \
    && rm -rf /var/lib/apt/lists/*

# Capture dumps (DEBUG=true) land in the working directory by default.
RUN mkdir /data && chown app:app /data
WORKDIR /data
USER app

# The base image presets port 8080 for ASP.NET. Both programs name their own
# ports, and would warn at every start that they override the preset.
ENV ASPNETCORE_HTTP_PORTS=

# The licences of the copied and ported code ask for their notices to travel
# with every copy, the image included.
COPY --from=builder /src/THIRD_PARTY_NOTICES.md /usr/share/doc/ninjacat/
COPY --from=builder /src/src/Proto/protos /usr/share/doc/ninjacat/protos

FROM runtime AS monitors

COPY --from=publish-monitors /out /app

# 2552 Akka remoting, 8558 Akka.Management, 8559 health.
EXPOSE 2552 8558 8559

ENTRYPOINT ["/app/ninjacat-monitors"]

FROM runtime AS api

COPY --from=publish-api /out /app

# `intake` listens on 8080 (and 10516, logs over TCP), `query` on 8081.
EXPOSE 8080 8081 10516

# The command says which process this container is: `intake`, `query`, or
# `migrate` (apply the ClickHouse migrations and exit). There is no default.
ENTRYPOINT ["/app/ninjacat-api"]
