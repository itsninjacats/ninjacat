namespace NinjaCat.Api.Platform

open System
open System.Diagnostics
open System.Threading
open System.Threading.Tasks
open Microsoft.Extensions.Hosting
open Microsoft.Extensions.Logging
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

type ReadingKind =
    | Gauge
    /// `Read` returns a running total; what was added since the last sample is sent.
    | Count

/// A value a process reports about itself besides what every process
/// reports. A host registers each one as a service; `Read` runs once per
/// sample, on the sampling thread.
type Reading =
    { Name: string
      Unit: string
      Kind: ReadingKind
      Tags: Map<string, string[]>
      Read: unit -> float }

/// NinjaCat's own vital signs, stored as ordinary metrics so they can be
/// graphed and alerted on like any other: the process (`ninjacat.node.*`),
/// each table's writer (`ninjacat.storage.*`) and the readings its host adds.
type SelfMonitor(sink: ClickHouseSink, cfg: Config.Config, readings: Reading seq, log: ILogger<SelfMonitor>) =
    inherit BackgroundService()

    let host =
        if cfg.SelfMonitorHost <> "" then cfg.SelfMonitorHost else Environment.MachineName

    let interval = uint32 cfg.SelfMonitorInterval.TotalSeconds
    let tags = Map [ "runtime", [| ".NET " + string Environment.Version |] ]

    let point (now: DateTime) (metricType: string) (name: string) (unit: string) (value: float) (tags: Map<string, string[]>) : MetricPoint =
        { Metrics.point cfg.SelfMonitorTenant now name value with
            Host = host
            MetricType = metricType
            SourceType = "ninjacat"
            Unit = unit
            Interval = interval
            Tags = tags }

    /// Counters are sent as what was added since the last sample.
    let mutable previous: (TimeSpan * TimeSpan * TimeSpan * int * Map<string, WriterStats>) option = None

    let readings = Array.ofSeq readings
    let lastTotals: float option[] = Array.create readings.Length None

    // A reading is the host's code: one that throws or is not a number costs
    // its own point, never the sample or the process.
    let read (r: Reading) =
        try
            let value = r.Read()
            if Double.IsFinite value then Some value else None
        with ex ->
            log.LogWarning(ex, "Self-monitoring could not read {Metric}", r.Name)
            None

    let hostPoints (now: DateTime) =
        [ for i in 0 .. readings.Length - 1 do
              let r = readings.[i]
              let pointTags = Map.fold (fun acc k v -> Map.add k v acc) tags r.Tags

              match r.Kind, read r with
              | _, None -> ()
              | Gauge, Some value -> point now "GAUGE" r.Name r.Unit value pointTags
              | Count, Some total ->
                  match lastTotals.[i] with
                  | Some last -> point now "COUNT" r.Name r.Unit (max (total - last) 0.0) pointTags
                  | None -> ()

                  lastTotals.[i] <- Some total ]

    let sample () : MetricPoint[] =
        let now = DateTime.UtcNow
        let proc = Process.GetCurrentProcess()
        let gauge name unit value = point now "GAUGE" ("ninjacat.node." + name) unit value tags
        let count name unit value = point now "COUNT" ("ninjacat.node." + name) unit (max value 0.0) tags

        let collections = GC.CollectionCount 0 + GC.CollectionCount 1 + GC.CollectionCount 2
        let writers = sink.Stats |> List.map (fun w -> w.Table, w) |> Map.ofList

        let gauges =
            [ gauge "uptime" "second" (now - proc.StartTime.ToUniversalTime()).TotalSeconds
              gauge "threads" "" (float ThreadPool.ThreadCount)
              gauge "memory.used" "byte" (float proc.WorkingSet64)
              gauge "memory.alloc" "byte" (float (GC.GetTotalMemory false))
              gauge "heap.live" "byte" (float (GC.GetGCMemoryInfo().HeapSizeBytes)) ]
            @ [ for w in sink.Stats ->
                    point now "GAUGE" "ninjacat.storage.rows.buffered" "" (float w.Buffered) (tags.Add("table", [| w.Table |])) ]

        let counts =
            match previous with
            | None -> []
            | Some(userTime, systemTime, gcPause, lastCollections, lastWriters) ->
                [ count "gc.cycles" "" (float (collections - lastCollections))
                  count "cpu.gc_time" "second" (GC.GetTotalPauseDuration() - gcPause).TotalSeconds
                  count "cpu.user_time" "second" (proc.UserProcessorTime - userTime).TotalSeconds
                  count "cpu.system_time" "second" (proc.PrivilegedProcessorTime - systemTime).TotalSeconds ]
                @ [ for w in sink.Stats do
                        let last =
                            lastWriters.TryFind w.Table
                            |> Option.defaultValue { w with Written = 0L; Dropped = 0L; Failed = 0L }

                        let tableTags = tags.Add("table", [| w.Table |])
                        point now "COUNT" "ninjacat.storage.rows.written" "" (float (w.Written - last.Written)) tableTags
                        point now "COUNT" "ninjacat.storage.rows.dropped" "" (float (w.Dropped - last.Dropped)) tableTags
                        point now "COUNT" "ninjacat.storage.rows.failed" "" (float (w.Failed - last.Failed)) tableTags ]

        previous <- Some(proc.UserProcessorTime, proc.PrivilegedProcessorTime, GC.GetTotalPauseDuration(), collections, writers)
        Array.ofList (gauges @ counts @ hostPoints now)

    override _.ExecuteAsync(ct: CancellationToken) : Task =
        task {
            use timer = new PeriodicTimer(cfg.SelfMonitorInterval)

            try
                while not ct.IsCancellationRequested do
                    let! _ = timer.WaitForNextTickAsync ct
                    Sink.write (sink :> ISink) Metrics.table (sample ())
            with :? OperationCanceledException ->
                ()
        }
