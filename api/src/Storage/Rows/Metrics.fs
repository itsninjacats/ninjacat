namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

type MetricPoint =
    { TenantID: string
      Timestamp: DateTime
      Metric: string
      Host: string
      /// GAUGE, COUNT, RATE or UNSPECIFIED.
      MetricType: string
      SourceType: string
      Unit: string
      Interval: uint32
      Value: float
      Tags: Map<string, string[]>
      /// Datadog's provenance of the metric: which product and integration.
      OriginProduct: uint32
      OriginCategory: uint32
      OriginService: uint32
      /// Resources other than the host, by type ("device" → "eth0").
      Resources: Map<string, string>
      /// Keys of the payload with no column, JSON-encoded.
      Extra: Map<string, string> }

type SketchRow =
    { TenantID: string
      Timestamp: DateTime
      Metric: string
      Host: string
      Tags: Map<string, string[]>
      Count: uint64
      Min: float
      Max: float
      Avg: float
      Sum: float
      BucketKeys: int32[]
      BucketCounts: uint32[]
      OriginProduct: uint32
      OriginCategory: uint32
      OriginService: uint32
      Resources: Map<string, string>
      Extra: Map<string, string> }

module Metrics =
    let table: Table<MetricPoint> =
        Table.create
            "storage_metrics"
            "metrics"
            [ "tenant_id"; "timestamp"; "metric"; "host"; "metric_type"; "source_type"; "unit"; "interval"; "value"; "tags"
              "origin_product"; "origin_category"; "origin_service"; "resources"; "extra" ]
            (fun r ->
                [| r.TenantID; r.Timestamp; r.Metric; r.Host; r.MetricType; r.SourceType; r.Unit; r.Interval; r.Value; r.Tags
                   r.OriginProduct; r.OriginCategory; r.OriginService; r.Resources; r.Extra |])

    /// A point with only what every point has; the rest is empty.
    let point (tenant: string) (timestamp: DateTime) (metric: string) (value: float) : MetricPoint =
        { TenantID = tenant
          Timestamp = timestamp
          Metric = metric
          Host = ""
          MetricType = ""
          SourceType = ""
          Unit = ""
          Interval = 0u
          Value = value
          Tags = Map.empty
          OriginProduct = 0u
          OriginCategory = 0u
          OriginService = 0u
          Resources = Map.empty
          Extra = Map.empty }

module Sketches =
    let table: Table<SketchRow> =
        { Table.create
              "storage_sketches"
              "sketches"
              [ "tenant_id"; "timestamp"; "metric"; "host"; "tags"; "count"; "min"; "max"; "avg"; "sum"; "bucket_keys"; "bucket_counts"
                "origin_product"; "origin_category"; "origin_service"; "resources"; "extra" ]
              (fun r ->
                  [| r.TenantID; r.Timestamp; r.Metric; r.Host; r.Tags; r.Count; r.Min; r.Max; r.Avg; r.Sum; r.BucketKeys; r.BucketCounts
                     r.OriginProduct; r.OriginCategory; r.OriginService; r.Resources; r.Extra |])
          with
              MaxRows = 1000
              FlushInterval = TimeSpan.FromSeconds 5.0 }
