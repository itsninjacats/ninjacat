namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One AgentDiscoveryPayload. The batch's host id repeats onto every row.
type AgentDiscoveryRow =
    { TenantID: string
      ReceivedAt: DateTime
      HostID: string
      Integration: string
      Runtime: string
      RuntimeID: string
      IngestionTimestamp: DateTime option
      /// The four Config arrays are index-aligned.
      ConfigPaths: string[]
      /// `bytes` on the wire: a config file is not always UTF-8.
      ConfigContents: byte[][]
      ConfigTruncated: uint8[]
      ConfigFormats: string[]
      /// Names and VALUES, index-aligned. Arrays, not a map: env vars are a
      /// repeated field, so a name can occur twice and the order is the agent's.
      EnvVarNames: string[]
      EnvVarValues: string[] }

module AgentDiscovery =
    let table: Table<AgentDiscoveryRow> =
        { Table.create
              "storage_agent_discovery"
              "agent_discovery"
              [ "tenant_id"; "received_at"; "host_id"; "integration"; "runtime"; "runtime_id"
                "ingestion_timestamp"; "config_paths"; "config_contents"; "config_truncated"
                "config_formats"; "env_var_names"; "env_var_values" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.HostID; r.Integration; r.Runtime; r.RuntimeID
                     Col.opt r.IngestionTimestamp; r.ConfigPaths; r.ConfigContents; r.ConfigTruncated
                     r.ConfigFormats; r.EnvVarNames; r.EnvVarValues |])
          with
              // A row can carry whole config files.
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }

/// One HealthReport. The wire has no report id; ReportID is minted at ingest
/// and joins the report to its issues.
type AgentHealthReportRow =
    { TenantID: string
      ReceivedAt: DateTime
      ReportID: Guid
      SchemaVersion: string
      EventType: string
      EmittedAt: string
      EmittedAtParsed: DateTime option
      Service: string
      Host: string
      AgentVersion: string option
      ParIDs: string[]
      IssueCount: uint32 }

module AgentHealthReports =
    let table: Table<AgentHealthReportRow> =
        { Table.create
              "storage_agent_health_reports"
              "agent_health_reports"
              [ "tenant_id"; "received_at"; "report_id"; "schema_version"; "event_type"; "emitted_at"
                "emitted_at_parsed"; "service"; "host"; "agent_version"; "par_ids"; "issue_count" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.ReportID; r.SchemaVersion; r.EventType; r.EmittedAt
                     Col.opt r.EmittedAtParsed; r.Service; r.Host; Col.opt r.AgentVersion; r.ParIDs; r.IssueCount |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 5.0 }

/// One entry of a HealthReport's issues map.
type AgentHealthIssueRow =
    { TenantID: string
      ReceivedAt: DateTime
      ReportID: Guid
      IssueKey: string
      ID: string
      IssueName: string
      Title: string
      Description: string
      Category: string
      Location: string
      /// The enum's name.
      Severity: string
      DetectedAt: string
      DetectedAtParsed: DateTime option
      Source: string
      /// Schemaless per issue, as JSON text.
      Extra: string
      RemediationSummary: string
      RemediationStepOrder: int32[]
      RemediationStepText: string[]
      ScriptLanguage: string
      ScriptLanguageVersion: string
      ScriptFilename: string
      /// None when there is no script at all, which is not the same as a
      /// script that needs no root.
      ScriptRequiresRoot: uint8 option
      ScriptContent: string
      Tags: Map<string, string[]>
      /// The enum's name.
      PersistedState: string
      FirstSeen: string
      LastSeen: string
      ResolvedAt: string option
      IssueType: string }

module AgentHealthIssues =
    let table: Table<AgentHealthIssueRow> =
        { Table.create
              "storage_agent_health_issues"
              "agent_health_issues"
              [ "tenant_id"; "received_at"; "report_id"; "issue_key"; "id"; "issue_name"; "title"; "description"
                "category"; "location"; "severity"; "detected_at"; "detected_at_parsed"; "source"; "extra"
                "remediation_summary"; "remediation_step_order"; "remediation_step_text"; "script_language"
                "script_language_version"; "script_filename"; "script_requires_root"; "script_content"; "tags"
                "persisted_state"; "first_seen"; "last_seen"; "resolved_at"; "issue_type" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.ReportID; r.IssueKey; r.ID; r.IssueName; r.Title; r.Description
                     r.Category; r.Location; r.Severity; r.DetectedAt; Col.opt r.DetectedAtParsed; r.Source; r.Extra
                     r.RemediationSummary; r.RemediationStepOrder; r.RemediationStepText; r.ScriptLanguage
                     r.ScriptLanguageVersion; r.ScriptFilename; Col.opt r.ScriptRequiresRoot; r.ScriptContent; r.Tags
                     r.PersistedState; r.FirstSeen; r.LastSeen; Col.opt r.ResolvedAt; r.IssueType |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

/// One envelope from /api/v2/events.
type EventManagementEventRow =
    { TenantID: string
      ReceivedAt: DateTime
      DataType: string
      Host: string
      Title: string
      Category: string
      IntegrationID: string
      Message: string
      Timestamp: string
      TimestampParsed: DateTime option
      Tags: Map<string, string[]>
      AggregationKey: string
      NotableEventType: string
      /// The producer's own data.attributes.attributes object, as JSON text.
      Attributes: string
      /// Keys with no column, JSON per key.
      OuterExtra: Map<string, string> }

module EventManagement =
    let table: Table<EventManagementEventRow> =
        { Table.create
              "storage_event_management_events"
              "event_management_events"
              [ "tenant_id"; "received_at"; "data_type"; "host"; "title"; "category"; "integration_id"; "message"
                "timestamp"; "timestamp_parsed"; "tags"; "aggregation_key"; "notable_event_type"; "attributes"
                "outer_extra" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.DataType; r.Host; r.Title; r.Category; r.IntegrationID; r.Message
                     r.Timestamp; Col.opt r.TimestampParsed; r.Tags; r.AggregationKey; r.NotableEventType; r.Attributes
                     r.OuterExtra |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 2.0 }

/// One software entry of a softinv payload: a row per package, so "every
/// host running curl < 8.0" is a plain query.
type HostSoftwareRow =
    { TenantID: string
      ReceivedAt: DateTime
      Hostname: string
      SoftwareType: string
      Name: string
      Version: string
      Publisher: string
      DeploymentStatus: string
      DeploymentTime: string
      DeploymentTimeParsed: DateTime option
      ProductCode: string
      /// None when the agent never determined it; false would claim 32-bit.
      Is64Bit: uint8 option
      InstallPaths: string[]
      /// Keys of the entry with no column, JSON per key.
      Extra: Map<string, string>
      /// Keys of the payload with no column, repeated onto each of its rows.
      PayloadExtra: Map<string, string> }

module HostSoftware =
    let table: Table<HostSoftwareRow> =
        { Table.create
              "storage_host_software"
              "host_software"
              [ "tenant_id"; "received_at"; "hostname"; "software_type"; "name"; "version"; "publisher"
                "deployment_status"; "deployment_time"; "deployment_time_parsed"; "product_code"; "is_64_bit"
                "install_paths"; "extra"; "payload_extra" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.Hostname; r.SoftwareType; r.Name; r.Version; r.Publisher
                     r.DeploymentStatus; r.DeploymentTime; Col.opt r.DeploymentTimeParsed; r.ProductCode
                     Col.opt r.Is64Bit; r.InstallPaths; r.Extra; r.PayloadExtra |])
          with
              // A full inventory is a few hundred rows per host.
              MaxRows = 10_000
              FlushInterval = TimeSpan.FromSeconds 3.0
              BufferLimit = 100_000
              MaxInFlight = 2 }

/// One synthetic test result.
type SyntheticsResultRow =
    { TenantID: string
      ReceivedAt: DateTime
      TestID: string
      TestName: string
      TestType: string
      TestSubtype: string
      TestVersion: string
      LocationID: string
      LocationName: string
      LocationDisplayName: string
      ResultID: string
      ResultInitialID: string
      Status: string
      RunType: string
      /// The wire type is undocumented; kept as sent.
      Duration: string
      TestStartedAt: string
      TestStartedAtParsed: DateTime option
      TestFinishedAt: string
      TestFinishedAtParsed: DateTime option
      TestTriggeredAt: string
      TestTriggeredAtParsed: DateTime option
      /// The five Assertion arrays are index-aligned.
      AssertionType: string[]
      AssertionOperator: string[]
      AssertionExpected: string[]
      AssertionActual: string[]
      AssertionValid: uint8[]
      /// Both None when the result has no failure object.
      FailureCode: string option
      FailureMessage: string option
      Config: string
      NetstatsPacketsSent: int64 option
      NetstatsPacketsReceived: int64 option
      NetstatsPacketLossPercentage: float option
      NetstatsJitter: float option
      NetstatsLatency: float option
      NetstatsHops: int64 option
      Netpath: string
      DD: string
      Enrichment: string
      V: string
      Extra: Map<string, string> }

module SyntheticsResults =
    let table: Table<SyntheticsResultRow> =
        { Table.create
              "storage_synthetics_results"
              "synthetics_results"
              [ "tenant_id"; "received_at"; "test_id"; "test_name"; "test_type"; "test_subtype"; "test_version"
                "location_id"; "location_name"; "location_display_name"; "result_id"; "result_initial_id"; "status"
                "run_type"; "duration"; "test_started_at"; "test_started_at_parsed"; "test_finished_at"
                "test_finished_at_parsed"; "test_triggered_at"; "test_triggered_at_parsed"; "assertion_type"
                "assertion_operator"; "assertion_expected"; "assertion_actual"; "assertion_valid"; "failure_code"
                "failure_message"; "config"; "netstats_packets_sent"; "netstats_packets_received"
                "netstats_packet_loss_percentage"; "netstats_jitter"; "netstats_latency"; "netstats_hops"; "netpath"
                "dd"; "enrichment"; "v"; "extra" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.TestID; r.TestName; r.TestType; r.TestSubtype; r.TestVersion
                     r.LocationID; r.LocationName; r.LocationDisplayName; r.ResultID; r.ResultInitialID; r.Status
                     r.RunType; r.Duration; r.TestStartedAt; Col.opt r.TestStartedAtParsed; r.TestFinishedAt
                     Col.opt r.TestFinishedAtParsed; r.TestTriggeredAt; Col.opt r.TestTriggeredAtParsed; r.AssertionType
                     r.AssertionOperator; r.AssertionExpected; r.AssertionActual; r.AssertionValid; Col.opt r.FailureCode
                     Col.opt r.FailureMessage; r.Config; Col.opt r.NetstatsPacketsSent; Col.opt r.NetstatsPacketsReceived
                     Col.opt r.NetstatsPacketLossPercentage; Col.opt r.NetstatsJitter; Col.opt r.NetstatsLatency
                     Col.opt r.NetstatsHops; r.Netpath; r.DD; r.Enrichment; r.V; r.Extra |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

/// One OpenLineage RunEvent. Facets are the spec's own extension point, so
/// they stay JSON text.
type OpenLineageEventRow =
    { TenantID: string
      ReceivedAt: DateTime
      EventType: string
      EventTime: string
      EventTimeParsed: DateTime option
      Producer: string
      SchemaURL: string
      RunID: string
      RunFacets: string
      JobNamespace: string
      JobName: string
      JobFacets: string
      /// Index-aligned per list; inputs and outputs are independent.
      InputNamespace: string[]
      InputName: string[]
      InputFacets: string[]
      OutputNamespace: string[]
      OutputName: string[]
      OutputFacets: string[]
      APIVersion: string
      Via: string
      Extra: Map<string, string> }

module OpenLineage =
    let table: Table<OpenLineageEventRow> =
        { Table.create
              "storage_openlineage_events"
              "openlineage_events"
              [ "tenant_id"; "received_at"; "event_type"; "event_time"; "event_time_parsed"; "producer"; "schema_url"
                "run_id"; "run_facets"; "job_namespace"; "job_name"; "job_facets"; "input_namespace"; "input_name"
                "input_facets"; "output_namespace"; "output_name"; "output_facets"; "api_version"; "via"; "extra" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.EventType; r.EventTime; Col.opt r.EventTimeParsed; r.Producer; r.SchemaURL
                     r.RunID; r.RunFacets; r.JobNamespace; r.JobName; r.JobFacets; r.InputNamespace; r.InputName
                     r.InputFacets; r.OutputNamespace; r.OutputName; r.OutputFacets; r.APIVersion; r.Via; r.Extra |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

/// One entry of a query-actions batch. Nothing upstream names its fields, so
/// the entry is kept whole and Keys lists its top-level names.
type QueryActionResultRow =
    { TenantID: string
      ReceivedAt: DateTime
      Result: string
      Keys: string[]
      DDEVPOrigin: string
      DDEVPOriginVersion: string }

module QueryActionResults =
    let table: Table<QueryActionResultRow> =
        { Table.create
              "storage_query_action_results"
              "query_action_results"
              [ "tenant_id"; "received_at"; "result"; "keys"; "dd_evp_origin"; "dd_evp_origin_version" ]
              (fun r -> [| r.TenantID; r.ReceivedAt; r.Result; r.Keys; r.DDEVPOrigin; r.DDEVPOriginVersion |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 2.0 }
