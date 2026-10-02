namespace NinjaCat.Api.Server

open System
open System.Diagnostics
open System.Threading
open System.Threading.Tasks
open Microsoft.Extensions.Hosting
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// NinjaCat's own vital signs, stored as ordinary metrics so they can be
/// graphed and alerted on like any other: the process (`ninjacat.node.*`)
/// and each table's writer (`ninjacat.storage.*`).
type SelfMonitor(sink: ClickHouseSink, cfg: Config.Config) =
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
        Array.ofList (gauges @ counts)

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
