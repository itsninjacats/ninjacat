# Production image for the query service, built in two stages.
FROM mcr.microsoft.com/dotnet/sdk:10.0 AS builder

WORKDIR /src

# Project files first: the restore layer is reused until a dependency changes.
COPY global.json Directory.Build.props Directory.Packages.props NinjaCat.Api.slnx ./
COPY src/Engine/Engine.fsproj src/Engine/
COPY src/Server/Server.fsproj src/Server/
COPY tests/Engine.Tests/Engine.Tests.fsproj tests/Engine.Tests/
RUN dotnet restore src/Server/Server.fsproj

COPY . .
RUN dotnet publish src/Server/Server.fsproj -c Release --no-restore -o /out

# aspnet, not runtime-deps: the service is framework-dependent, which keeps
# the image layer shared with every other ASP.NET image on the host.
FROM mcr.microsoft.com/dotnet/aspnet:10.0

COPY --from=builder /out /app
WORKDIR /app
USER app

ENV NINJACAT_API_URL=http://0.0.0.0:8082
EXPOSE 8082

ENTRYPOINT ["dotnet", "ninjacat-api.dll"]
