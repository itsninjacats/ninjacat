/// Metric queries that need no parser: catalog lookups.
module NinjaCat.Api.Engine.Metrics

open System

/// Datadog's `GET /api/v1/metrics`: metrics reported since `From`, optionally by
/// one host.
type ActiveMetricsRequest =
    { Tenant: TenantId
      From: DateTimeOffset
      Host: string option }

let activeMetrics (req: ActiveMetricsRequest) : Sql =
    let (TenantId tenant) = req.Tenant
    let tenantP = SqlValue.String tenant
    let fromP = SqlValue.Int64(req.From.ToUnixTimeSeconds())

    let hostFilter, hostParams =
        match req.Host with
        | Some h ->
            let p = SqlValue.String h
            let placeholder = Sql.param "host" p
            $"\n  AND host = {placeholder}", [ "host", p ]
        | None -> "", []

    // Raw `metrics` rather than `metrics_1m`: the rollup is per minute, so a
    // `from` inside the last minute would miss metrics that just appeared.
    // DISTINCT over (tenant_id, metric) walks the primary key prefix and stays
    // cheap as long as the time filter prunes partitions.
    { Text =
        $"""SELECT DISTINCT metric
FROM metrics
WHERE tenant_id = {Sql.param "tenant" tenantP}
  AND timestamp >= fromUnixTimestamp({Sql.param "from" fromP}){hostFilter}
ORDER BY metric"""
      Parameters = [ "tenant", tenantP; "from", fromP ] @ hostParams }
