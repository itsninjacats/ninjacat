namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One instrumentation telemetry request, or one entry of a message-batch
/// request (see BatchIndex).
type APMTelemetryRow =
    { TenantID: string
      ReceivedAt: DateTime
      RequestType: string
      Producer: string
      APIVersion: string
      RuntimeID: string
      SeqID: int64 option
      /// The parsed Unix time; None when nothing was sent, never the epoch.
      /// The *Raw twin keeps the JSON text, so a value that did not parse is
      /// still there.
      TracerTime: DateTime option
      TracerTimeRaw: string
      EventTime: DateTime option
      EventTimeRaw: string
      ServiceName: string
      ServiceVersion: string
      Env: string
      LanguageName: string
      LanguageVersion: string
      TracerVersion: string
      Hostname: string
      HostOS: string
      HostArchitecture: string
      /// The host block's other keys, as JSON text by name.
      HostExtra: Map<string, string>
      /// The producer's own part, as it arrived. On the parent row of a
      /// batch this is the whole batch.
      Payload: string
      Debug: string
      Origin: string
      // Headers the trace-agent's proxy adds, never the sender itself.
      Via: string
      DDAgentHostname: string
      DDAgentEnv: string
      DatadogContainerID: string
      XDatadogContainerTags: string
      /// The envelope's keys with no column, as JSON text by name.
      Extra: Map<string, string>
      /// None on the request's own row; the entry's position on a row made
      /// from a message-batch entry. Entry 0 is not "no entry".
      BatchIndex: uint32 option
      /// "message-batch" on those rows, "" otherwise.
      ParentRequestType: string }

module ApmTelemetry =
    let table: Table<APMTelemetryRow> =
        { Table.create
              "storage_apm_telemetry"
              "apm_telemetry"
              [ "tenant_id"; "received_at"; "request_type"; "producer"; "api_version"; "runtime_id"
                "seq_id"; "tracer_time"; "tracer_time_raw"; "event_time"; "event_time_raw"
                "service_name"; "service_version"; "env"; "language_name"; "language_version"; "tracer_version"
                "hostname"; "host_os"; "host_architecture"; "host_extra"
                "payload"; "debug"; "origin"
                "via"; "dd_agent_hostname"; "dd_agent_env"; "datadog_container_id"; "x_datadog_container_tags"
                "extra"; "batch_index"; "parent_request_type" ]
              (fun (r: APMTelemetryRow) ->
                  [| r.TenantID; r.ReceivedAt; r.RequestType; r.Producer; r.APIVersion; r.RuntimeID
                     Col.opt r.SeqID; Col.opt r.TracerTime; r.TracerTimeRaw; Col.opt r.EventTime; r.EventTimeRaw
                     r.ServiceName; r.ServiceVersion; r.Env; r.LanguageName; r.LanguageVersion; r.TracerVersion
                     r.Hostname; r.HostOS; r.HostArchitecture; r.HostExtra
                     r.Payload; r.Debug; r.Origin
                     r.Via; r.DDAgentHostname; r.DDAgentEnv; r.DatadogContainerID; r.XDatadogContainerTags
                     r.Extra; Col.opt r.BatchIndex; r.ParentRequestType |])
          with
              // Every instrumented process heartbeats on its own schedule, so
              // a deploy makes many of them report within the same second.
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }
