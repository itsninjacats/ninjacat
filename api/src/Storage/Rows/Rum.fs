/// RUM: everything the browser, iOS and Android SDKs send to browser-intake.
///
/// Every row carries the whole payload it came from (Event, Segment, Span).
/// The RUM schema has over a hundred optional fields per variant and grows
/// with every SDK release, so the typed columns are a filtering index over
/// the event, and the event is the record.
namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// The sender columns every RUM table starts with. A RUM event has no
/// hostname, agent or container id: the request it arrived in is the only
/// thing that says who sent it.
type RumRequest =
    { TenantID: string
      ReceivedAt: DateTime
      DDSource: string
      EVPOrigin: string
      EVPOriginVersion: string
      /// How the batch arrived compressed, as the sender spelled it. The body
      /// is stored decoded, so nothing else in the row remembers.
      EVPEncoding: string
      RequestID: string
      IdempotencyKey: string
      /// Browser only: the transport, "fetch" or "beacon". A beacon is fire
      /// and forget at page unload, so a gap in beacon traffic means
      /// something different from a gap in fetch traffic.
      DDAPI: string
      BatchTime: DateTime option
      /// None on a first attempt: "never retried" and "retried zero times"
      /// are different facts.
      RetryCount: uint32 option
      RetryAfter: int64 option
      RemoteAddr: string
      UserAgent: string
      /// Query parameters without a column of their own, as JSON, credentials
      /// removed.
      QueryExtra: string }

module RumRequest =
    let columns: string list =
        [ "tenant_id"; "received_at"; "ddsource"; "evp_origin"; "evp_origin_version"; "evp_encoding"
          "request_id"; "idempotency_key"; "dd_api"; "batch_time"; "retry_count"; "retry_after"
          "remote_addr"; "user_agent"; "query_extra" ]

    let values (q: RumRequest) : obj[] =
        [| q.TenantID; q.ReceivedAt; q.DDSource; q.EVPOrigin; q.EVPOriginVersion; q.EVPEncoding
           q.RequestID; q.IdempotencyKey; q.DDAPI; Col.opt q.BatchTime; Col.opt q.RetryCount; Col.opt q.RetryAfter
           q.RemoteAddr; q.UserAgent; q.QueryExtra |]

/// One view, upserted on (application, session, view) with DocumentVersion
/// as the ReplacingMergeTree version.
///
/// Counters and Web Vitals are options throughout: a view with no crash
/// object has not measured zero crashes, and a 0 would be averaged.
type RumViewRow =
    { Req: RumRequest
      Date: DateTime
      ApplicationID: string
      SessionID: string
      ViewID: string
      DocumentVersion: uint64
      /// "view" or "view_update": which wire form produced this version.
      EventType: string
      Service: string
      Version: string
      BuildVersion: string
      BuildID: string
      Source: string
      SessionType: string
      SessionHasReplay: bool option
      SessionIsActive: bool option
      SessionSampledForReplay: bool option
      UsrID: string
      UsrName: string
      UsrEmail: string
      UsrAnonymousID: string
      AccountID: string
      AccountName: string
      ViewURL: string
      ViewName: string
      ViewReferrer: string
      ViewLoadingType: string
      ViewLoadingTime: int64 option
      ViewTimeSpent: int64
      ViewIsActive: bool option
      ViewIsSlowRendered: bool option
      ActionCount: int64 option
      ErrorCount: int64 option
      CrashCount: int64 option
      LongTaskCount: int64 option
      FrozenFrameCount: int64 option
      ResourceCount: int64 option
      FrustrationCount: int64 option
      LCP: int64 option
      CLS: float option
      INP: int64 option
      FCP: int64 option
      FID: int64 option
      FBC: int64 option
      TTFB: int64 option
      DeviceType: string
      DeviceBrand: string
      DeviceModel: string
      DeviceName: string
      OSName: string
      OSVersion: string
      ConnectivityStatus: string
      /// JSON text: free-form maps whose values are arbitrary JSON.
      Context: string
      FeatureFlags: string
      Tags: Map<string, string[]>
      /// The whole event, as it arrived.
      Event: string }

module RumViews =
    let table: Table<RumViewRow> =
        { Table.create
              "storage_rum_views"
              "rum_views"
              (RumRequest.columns
               @ [ "date"; "application_id"; "session_id"; "view_id"; "document_version"; "event_type"
                   "service"; "version"; "build_version"; "build_id"; "source"
                   "session_type"; "session_has_replay"; "session_is_active"; "session_sampled_for_replay"
                   "usr_id"; "usr_name"; "usr_email"; "usr_anonymous_id"; "account_id"; "account_name"
                   "view_url"; "view_name"; "view_referrer"
                   "view_loading_type"; "view_loading_time"; "view_time_spent"; "view_is_active"
                   "view_is_slow_rendered"
                   "action_count"; "error_count"; "crash_count"; "long_task_count"; "frozen_frame_count"
                   "resource_count"; "frustration_count"
                   "lcp"; "cls"; "inp"; "fcp"; "fid"; "fbc"; "ttfb"
                   "device_type"; "device_brand"; "device_model"; "device_name"; "os_name"; "os_version"
                   "connectivity_status"; "context"; "feature_flags"; "ddtags"; "event" ])
              (fun r ->
                  Array.append
                      (RumRequest.values r.Req)
                      [| r.Date; r.ApplicationID; r.SessionID; r.ViewID; r.DocumentVersion; r.EventType
                         r.Service; r.Version; r.BuildVersion; r.BuildID; r.Source
                         r.SessionType; Col.opt r.SessionHasReplay; Col.opt r.SessionIsActive; Col.opt r.SessionSampledForReplay
                         r.UsrID; r.UsrName; r.UsrEmail; r.UsrAnonymousID; r.AccountID; r.AccountName
                         r.ViewURL; r.ViewName; r.ViewReferrer
                         r.ViewLoadingType; Col.opt r.ViewLoadingTime; r.ViewTimeSpent; Col.opt r.ViewIsActive
                         Col.opt r.ViewIsSlowRendered
                         Col.opt r.ActionCount; Col.opt r.ErrorCount; Col.opt r.CrashCount; Col.opt r.LongTaskCount
                         Col.opt r.FrozenFrameCount; Col.opt r.ResourceCount; Col.opt r.FrustrationCount
                         Col.opt r.LCP; Col.opt r.CLS; Col.opt r.INP; Col.opt r.FCP; Col.opt r.FID; Col.opt r.FBC; Col.opt r.TTFB
                         r.DeviceType; r.DeviceBrand; r.DeviceModel; r.DeviceName; r.OSName; r.OSVersion
                         r.ConnectivityStatus; r.Context; r.FeatureFlags; r.Tags; r.Event |])
          with
              // An active page re-sends its view every few seconds and every
              // row carries the whole event, so the limits stand for memory.
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 3.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

/// One action, error, resource, long_task, vital or transition. The columns
/// of the other kinds stay empty; EventType says which kind it is.
type RumEventRow =
    { Req: RumRequest
      Date: DateTime
      ApplicationID: string
      SessionID: string
      ViewID: string
      EventType: string
      Service: string
      Version: string
      BuildVersion: string
      BuildID: string
      Source: string
      SessionType: string
      SessionHasReplay: bool option
      UsrID: string
      UsrName: string
      UsrEmail: string
      UsrAnonymousID: string
      AccountID: string
      AccountName: string
      ViewURL: string
      ViewName: string
      ViewReferrer: string
      DeviceType: string
      DeviceBrand: string
      DeviceModel: string
      DeviceName: string
      OSName: string
      OSVersion: string
      ConnectivityStatus: string
      ActionType: string
      ActionName: string
      ActionID: string
      /// One click can be classified several ways at once (rage and dead).
      ActionFrustrationTypes: string[]
      ErrorID: string
      ErrorMessage: string
      ErrorType: string
      ErrorSource: string
      ErrorStack: string
      ErrorIsCrash: bool option
      ErrorHandling: string
      ErrorFingerprint: string
      ResourceID: string
      ResourceType: string
      ResourceURL: string
      ResourceMethod: string
      ResourceStatusCode: int64 option
      ResourceDuration: int64 option
      ResourceSize: int64 option
      LongTaskID: string
      LongTaskDuration: int64 option
      VitalID: string
      VitalType: string
      VitalName: string
      VitalDuration: int64 option
      Context: string
      Tags: Map<string, string[]>
      Event: string }

module RumEvents =
    let table: Table<RumEventRow> =
        { Table.create
              "storage_rum_events"
              "rum_events"
              (RumRequest.columns
               @ [ "date"; "application_id"; "session_id"; "view_id"; "event_type"
                   "service"; "version"; "build_version"; "build_id"; "source"
                   "session_type"; "session_has_replay"
                   "usr_id"; "usr_name"; "usr_email"; "usr_anonymous_id"; "account_id"; "account_name"
                   "view_url"; "view_name"; "view_referrer"
                   "device_type"; "device_brand"; "device_model"; "device_name"; "os_name"; "os_version"
                   "connectivity_status"
                   "action_type"; "action_name"; "action_id"; "action_frustration_types"
                   "error_id"; "error_message"; "error_type"; "error_source"; "error_stack"
                   "error_is_crash"; "error_handling"; "error_fingerprint"
                   "resource_id"; "resource_type"; "resource_url"; "resource_method"
                   "resource_status_code"; "resource_duration"; "resource_size"
                   "long_task_id"; "long_task_duration"
                   "vital_id"; "vital_type"; "vital_name"; "vital_duration"
                   "context"; "ddtags"; "event" ])
              (fun r ->
                  Array.append
                      (RumRequest.values r.Req)
                      [| r.Date; r.ApplicationID; r.SessionID; r.ViewID; r.EventType
                         r.Service; r.Version; r.BuildVersion; r.BuildID; r.Source
                         r.SessionType; Col.opt r.SessionHasReplay
                         r.UsrID; r.UsrName; r.UsrEmail; r.UsrAnonymousID; r.AccountID; r.AccountName
                         r.ViewURL; r.ViewName; r.ViewReferrer
                         r.DeviceType; r.DeviceBrand; r.DeviceModel; r.DeviceName; r.OSName; r.OSVersion
                         r.ConnectivityStatus
                         r.ActionType; r.ActionName; r.ActionID; r.ActionFrustrationTypes
                         r.ErrorID; r.ErrorMessage; r.ErrorType; r.ErrorSource; r.ErrorStack
                         Col.opt r.ErrorIsCrash; r.ErrorHandling; r.ErrorFingerprint
                         r.ResourceID; r.ResourceType; r.ResourceURL; r.ResourceMethod
                         Col.opt r.ResourceStatusCode; Col.opt r.ResourceDuration; Col.opt r.ResourceSize
                         r.LongTaskID; Col.opt r.LongTaskDuration
                         r.VitalID; r.VitalType; r.VitalName; Col.opt r.VitalDuration
                         r.Context; r.Tags; r.Event |])
          with
              // The high-volume table of the family: resources alone are
              // dozens per page load, each row with its whole event.
              MaxRows = 10000
              FlushInterval = TimeSpan.FromSeconds 2.0
              BufferLimit = 100_000
              MaxInFlight = 4 }

/// One type:"telemetry" event: the SDK reporting on itself.
type RumTelemetryRow =
    { Req: RumRequest
      Date: DateTime
      ApplicationID: string
      SessionID: string
      ViewID: string
      ActionID: string
      Type: string
      Status: string
      TelemetryType: string
      Message: string
      ErrorStack: string
      ErrorKind: string
      /// The whole telemetry.configuration object, as JSON.
      Configuration: string
      UsageFeature: string
      Service: string
      Version: string
      Source: string
      EffectiveSampleRate: float option
      ExperimentalFeatures: string[]
      DeviceBrand: string
      DeviceModel: string
      DeviceArchitecture: string
      OSName: string
      OSVersion: string
      OSBuild: string
      Event: string }

module RumTelemetry =
    let table: Table<RumTelemetryRow> =
        { Table.create
              "storage_rum_telemetry"
              "rum_telemetry"
              (RumRequest.columns
               @ [ "date"; "application_id"; "session_id"; "view_id"; "action_id"
                   "type"; "status"; "telemetry_type"
                   "message"; "error_stack"; "error_kind"; "configuration"; "usage_feature"
                   "service"; "version"; "source"
                   "effective_sample_rate"; "experimental_features"
                   "device_brand"; "device_model"; "device_architecture"
                   "os_name"; "os_version"; "os_build"
                   "event" ])
              (fun r ->
                  Array.append
                      (RumRequest.values r.Req)
                      [| r.Date; r.ApplicationID; r.SessionID; r.ViewID; r.ActionID
                         r.Type; r.Status; r.TelemetryType
                         r.Message; r.ErrorStack; r.ErrorKind; r.Configuration; r.UsageFeature
                         r.Service; r.Version; r.Source
                         Col.opt r.EffectiveSampleRate; r.ExperimentalFeatures
                         r.DeviceBrand; r.DeviceModel; r.DeviceArchitecture
                         r.OSName; r.OSVersion; r.OSBuild
                         r.Event |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

/// One mobile cpu or memory sample run. Timestamps and values stay the
/// parallel arrays they arrived as: timestamps[i] belongs to values.*[i].
type RumTimeseriesRow =
    { Req: RumRequest
      Date: DateTime
      ApplicationID: string
      SessionID: string
      ViewID: string
      Name: string
      ID: string
      Schema: string
      Start: UnixNanos
      End: UnixNanos
      Timestamps: int64[]
      /// data.values, as JSON: a different object per variant.
      Values: string
      Service: string
      Version: string
      Source: string
      Context: string
      Tags: Map<string, string[]>
      Event: string }

module RumTimeseries =
    let table: Table<RumTimeseriesRow> =
        { Table.create
              "storage_rum_timeseries"
              "rum_timeseries"
              (RumRequest.columns
               @ [ "date"; "application_id"; "session_id"; "view_id"
                   "name"; "timeseries_id"; "timeseries_schema"
                   "start"; "end"; "timestamps"; "values"
                   "service"; "version"; "source"; "context"; "ddtags"; "event" ])
              (fun r ->
                  Array.append
                      (RumRequest.values r.Req)
                      [| r.Date; r.ApplicationID; r.SessionID; r.ViewID
                         r.Name; r.ID; r.Schema
                         r.Start; r.End; r.Timestamps; r.Values
                         r.Service; r.Version; r.Source; r.Context; r.Tags; r.Event |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

/// One blob part of a /api/v2/replay upload, with the metadata entry that
/// described it. The bytes are stored exactly as received: the SDK emits
/// zlib streams built to be concatenated later.
type RumReplaySegmentRow =
    { Req: RumRequest
      ApplicationID: string
      SessionID: string
      ViewID: string
      Source: string
      /// "segment" or "resource".
      Variant: string
      PartName: string
      PartFilename: string
      Start: DateTime option
      End: DateTime option
      RecordsCount: int64 option
      HasFullSnapshot: bool option
      IndexInView: int64 option
      CreationReason: string
      RawSegmentSize: int64 option
      CompressedSegmentSize: int64 option
      Segment: byte[]
      Image: byte[]
      Event: string }

module RumReplaySegments =
    let table: Table<RumReplaySegmentRow> =
        { Table.create
              "storage_rum_replay_segments"
              "rum_replay_segments"
              (RumRequest.columns
               @ [ "application_id"; "session_id"; "view_id"; "source"
                   "variant"; "part_name"; "part_filename"
                   "start"; "end"; "records_count"; "has_full_snapshot"; "index_in_view"; "creation_reason"
                   "raw_segment_size"; "compressed_segment_size"
                   "segment"; "image"; "event" ])
              (fun r ->
                  Array.append
                      (RumRequest.values r.Req)
                      [| r.ApplicationID; r.SessionID; r.ViewID; r.Source
                         r.Variant; r.PartName; r.PartFilename
                         Col.opt r.Start; Col.opt r.End; Col.opt r.RecordsCount; Col.opt r.HasFullSnapshot
                         Col.opt r.IndexInView; r.CreationReason
                         Col.opt r.RawSegmentSize; Col.opt r.CompressedSegmentSize
                         r.Segment; r.Image; r.Event |])
          with
              // A row is a compressed recording segment: these counts stand
              // for megabytes.
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }

/// One span out of a /api/v2/spans envelope.
///
/// The three ids are text. The mobile SDKs write them as decimal or hex text
/// of 64- and 128-bit numbers; parsing would need a guess about the base.
type RumSpanRow =
    { Req: RumRequest
      TraceID: string
      SpanID: string
      ParentID: string
      Name: string
      Service: string
      Resource: string
      Type: string
      /// From the envelope around the span, not from the span.
      Env: string
      Start: UnixNanos
      DurationNS: int64
      Error: sbyte
      /// meta's nested objects (device, os) arrive flattened to dotted keys.
      Meta: Map<string, string>
      Metrics: Map<string, float>
      Span: string
      /// Envelope keys other than "spans" and "env", as JSON.
      EnvelopeExtra: string }

module RumSpans =
    let table: Table<RumSpanRow> =
        { Table.create
              "storage_rum_spans"
              "rum_spans"
              (RumRequest.columns
               @ [ "trace_id"; "span_id"; "parent_id"
                   "name"; "service"; "resource"; "type"; "env"
                   "start"; "duration_ns"; "error"; "meta"; "metrics"; "span"; "envelope_extra" ])
              (fun r ->
                  Array.append
                      (RumRequest.values r.Req)
                      [| r.TraceID; r.SpanID; r.ParentID
                         r.Name; r.Service; r.Resource; r.Type; r.Env
                         r.Start; r.DurationNS; r.Error; r.Meta; r.Metrics; r.Span; r.EnvelopeExtra |])
          with
              MaxRows = 5000
              FlushInterval = TimeSpan.FromSeconds 2.0 }
