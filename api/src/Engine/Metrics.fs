/// Metric queries that need no parser: catalog lookups.
module NinjaCat.Api.Engine.Metrics

open System

/// The tags `metrics` also keeps as columns of their own, each the whole list
/// of the tag's values (0005). The rollup has none of them.
let liftedTags = set [ "env"; "service"; "kube_namespace"; "kube_deployment"; "pod_name" ]

/// Points of `metrics` from a SQL expression on. The table is sorted by the
/// 15-minute bucket before the series, and ClickHouse does not derive the
/// bucket from a condition on `timestamp`: without the bucket condition a
/// query reads every granule of its metric.
let rawSince (from: string) : string =
    $"timestamp >= {from} AND toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes({from})"

/// Points of `metrics` between two SQL expressions, the bucket stated as in
/// rawSince.
let rawWindow (from: string) (until: string) : string =
    rawSince from + $" AND timestamp < {until} AND toStartOfFifteenMinutes(timestamp) < {until}"

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

    let from = $"""fromUnixTimestamp({Sql.param "from" fromP})"""

    // Raw `metrics` rather than `metrics_1m`: the rollup is per minute, so a
    // `from` inside the last minute would miss metrics that just appeared.
    { Text =
        $"""SELECT DISTINCT metric
FROM metrics
WHERE tenant_id = {Sql.param "tenant" tenantP}
  AND {rawSince from}{hostFilter}
ORDER BY metric"""
      Parameters = [ "tenant", tenantP; "from", fromP ] @ hostParams }
