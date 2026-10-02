/// CI Visibility, CI provider webhooks, git metadata and the synthetics test
/// catalogue. One product, but separate tables: each is a different wire
/// format.
namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One event of a citestcycle payload: a test, or the end of a suite, module
/// or session.
type CITestEventRow =
    { TenantID: string
      ReceivedAt: DateTime
      EventType: string
      EventVersion: int32
      PayloadVersion: int32
      /// Zero when the event type carries no such id: that is the tracer's
      /// own spelling of "none".
      SessionID: uint64
      ModuleID: uint64
      SuiteID: uint64
      TraceID: uint64
      SpanID: uint64
      ParentID: uint64
      ITRCorrelationID: string
      Service: string
      Env: string
      Name: string
      Resource: string
      SpanType: string
      Start: UnixNanos
      DurationNs: int64
      Error: int32
      TestName: string
      TestSuite: string
      TestModule: string
      TestFramework: string
      TestFrameworkVersion: string
      TestStatus: string
      TestType: string
      TestSourceFile: string
      TestSourceStart: int64 option
      TestSourceEnd: int64 option
      TestParameters: string
      TestCodeowners: string
      TestCommand: string
      TestSessionName: string
      GitRepositoryURL: string
      GitBranch: string
      GitTag: string
      GitCommitSHA: string
      GitCommitMessage: string
      GitCommitAuthorName: string
      GitCommitAuthorEmail: string
      GitCommitAuthorDate: string
      GitCommitCommitterName: string
      GitCommitCommitterEmail: string
      GitCommitCommitterDate: string
      CIProviderName: string
      CIPipelineID: string
      CIPipelineName: string
      CIPipelineNumber: string
      CIPipelineURL: string
      CIJobID: string
      CIJobName: string
      CIJobURL: string
      CIStageName: string
      CIWorkspacePath: string
      CINodeName: string
      CINodeLabels: string
      OSPlatform: string
      OSVersion: string
      OSArchitecture: string
      RuntimeName: string
      RuntimeVersion: string
      Language: string
      RuntimeID: string
      LibraryVersion: string
      /// The flags are optional because absent and false differ: absent means
      /// the tracer has the feature off, false that it is on and said no.
      TestIsNew: uint8 option
      TestIsRetry: uint8 option
      TestIsModified: uint8 option
      TestSkippedByITR: uint8 option
      ITRUnskippable: uint8 option
      ITRForcedRun: uint8 option
      CodeCoverageEnabled: uint8 option
      TestRetryReason: string
      EarlyFlakeAbortReason: string
      EVPSubdomain: string
      ContainerID: string
      AgentHostname: string
      AgentVersion: string
      Meta: Map<string, string>
      Metrics: Map<string, float>
      Metadata: Map<string, string>
      /// The event's content as JSON, whatever format the request was in.
      Content: string }

/// One entry of a citestcov payload's coverages[].
type CICoverageRow =
    { TenantID: string
      ReceivedAt: DateTime
      PayloadVersion: int32
      SessionID: uint64
      SuiteID: uint64
      /// Optional because the encoder leaves it out in suite-skipping mode,
      /// and 0 is a legal span id.
      SpanID: uint64 option
      FilesFilename: string[]
      /// Parallel to FilesFilename: a file without line data keeps an empty
      /// bitmap at its index. The bitmaps are opaque bytes.
      FilesBitmap: byte[][]
      /// This entry's bytes as they arrived; RawFormat is "msgpack" or "json".
      Raw: byte[]
      RawFormat: string
      Event: byte[]
      Extra: Map<string, string>
      EVPSubdomain: string
      ContainerID: string
      AgentHostname: string
      AgentVersion: string }

/// A sha that was mentioned for a repository. Not a claim that its objects
/// are held: most rows come from a tracer asking what we have. PackfileID is
/// set only when the sha arrived with a packfile.
type GitCommitRow =
    { TenantID: string
      RepositoryURL: string
      SHA: string
      SeenAt: DateTime
      PackfileID: string option
      Source: string }

/// One .pack file upload.
type GitPackfileRow =
    { TenantID: string
      ReceivedAt: DateTime
      RepositoryURL: string
      PushedSHA: string
      /// SHA-256 of the pack, hex: the upload has no id of its own, and
      /// git_commits.packfile_id has to point at something.
      PackfileID: string
      Filename: string
      Packfile: byte[]
      SizeBytes: uint64
      OtherParts: Map<string, byte[]> }

/// One question a tracer asked about its configuration, and the answer it
/// got. The answer is kept because it is a contract of ours: the day it
/// changes, what a tracer was told then is what explains its behaviour.
type CISettingsRequestRow =
    { TenantID: string
      ReceivedAt: DateTime
      Endpoint: string
      RequestID: string
      RequestType: string
      Service: string
      Env: string
      RepositoryURL: string
      Branch: string
      SHA: string
      TestLevel: string
      Module: string
      CommitMessage: string
      PageState: string
      Configurations: string
      RequestBody: byte[]
      ResponseBody: string }

/// One element of a CI webhook batch: a pipeline, a stage or a job, told
/// apart by Level.
///
/// The ids are strings although Jenkins sends numbers: other providers put
/// their own identifiers in the same fields, and some are UUIDs.
type CIWebhookEventRow =
    { TenantID: string
      ReceivedAt: DateTime
      Provider: string
      Level: string
      Service: string
      PayloadVersion: int64 option
      PartialRetry: uint8 option
      IsManual: uint8 option
      TraceID: string
      SpanID: string
      ParentSpanID: string
      ID: string
      UniqueID: string
      PipelineID: string
      PipelineUniqueID: string
      PipelineName: string
      StageID: string
      StageName: string
      ParentStageID: string
      Name: string
      URL: string
      Status: string
      StartRaw: string
      StartParsed: DateTime option
      EndRaw: string
      EndParsed: DateTime option
      QueueTimeMs: int64 option
      NodeName: string
      NodeHostname: string
      NodeWorkspace: string
      NodeLabels: string[]
      GitRepositoryURL: string
      GitDefaultBranch: string
      GitBranch: string
      GitSHA: string
      GitTag: string
      GitMessage: string
      GitAuthorName: string
      GitAuthorEmail: string
      GitAuthorTime: string
      GitCommitterName: string
      GitCommitterEmail: string
      GitCommitTime: string
      UserName: string
      UserEmail: string
      ErrorMessage: string
      ErrorType: string
      ErrorDomain: string
      ErrorStack: string
      ParentPipelineTraceID: string
      ParentPipelineURL: string
      Parameters: Map<string, string>
      /// A tag multiset: the Jenkins plugin repeats keys.
      Tags: Map<string, string[]>
      DeliveryID: string
      Headers: Map<string, string>
      Extra: Map<string, string>
      Body: string }

/// One datadog-ci tag / measure / custom-span call.
type CIPipelineEventRow =
    { TenantID: string
      ReceivedAt: DateTime
      Kind: string
      DataType: string
      Provider: string
      CILevel: int64 option
      CIEnv: Map<string, string>
      /// A plain map: on this wire tags are a JSON object, so keys are unique.
      Tags: Map<string, string>
      Metrics: Map<string, float>
      SpanName: string
      SpanStartRaw: string
      SpanEndRaw: string
      Attributes: string
      Body: byte[]
      Extra: Map<string, string> }

/// One synthetic test the agent's poller may be handed. Nothing writes these
/// yet: the table is the catalogue a future panel fills.
type SyntheticsTestConfigRow =
    { TenantID: string
      UpdatedAt: DateTime
      PublicID: string
      Version: int32
      Type: string
      /// Only "UDP", "TCP" and "ICMP" are valid: the agent fails its whole
      /// poll on a subtype it does not know.
      Subtype: string
      TickEvery: int32
      OrgID: int64
      MainDC: string
      ResultID: string
      RunType: string
      Assertions: string
      Request: string
      Enabled: uint8 }

module CITestEvents =
    let table: Table<CITestEventRow> =
        { Table.create
              "storage_ci_test_events"
              "ci_test_events"
              [ "tenant_id"; "received_at"; "event_type"; "event_version"; "payload_version"
                "session_id"; "module_id"; "suite_id"; "trace_id"; "span_id"; "parent_id"
                "itr_correlation_id"; "service"; "env"; "name"; "resource"; "span_type"
                "start"; "duration_ns"; "error"
                "test_name"; "test_suite"; "test_module"; "test_framework"; "test_framework_version"
                "test_status"; "test_type"; "test_source_file"; "test_source_start"; "test_source_end"
                "test_parameters"; "test_codeowners"; "test_command"; "test_session_name"
                "git_repository_url"; "git_branch"; "git_tag"; "git_commit_sha"; "git_commit_message"
                "git_commit_author_name"; "git_commit_author_email"; "git_commit_author_date"
                "git_commit_committer_name"; "git_commit_committer_email"; "git_commit_committer_date"
                "ci_provider_name"; "ci_pipeline_id"; "ci_pipeline_name"; "ci_pipeline_number"; "ci_pipeline_url"
                "ci_job_id"; "ci_job_name"; "ci_job_url"; "ci_stage_name"; "ci_workspace_path"
                "ci_node_name"; "ci_node_labels"
                "os_platform"; "os_version"; "os_architecture"; "runtime_name"; "runtime_version"
                "language"; "runtime_id"; "library_version"
                "test_is_new"; "test_is_retry"; "test_is_modified"; "test_skipped_by_itr"
                "itr_unskippable"; "itr_forced_run"; "code_coverage_enabled"
                "test_retry_reason"; "early_flake_abort_reason"
                "evp_subdomain"; "container_id"; "agent_hostname"; "agent_version"
                "meta"; "metrics"; "metadata"; "content" ]
              (fun (r: CITestEventRow) ->
                  [| r.TenantID; r.ReceivedAt; r.EventType; r.EventVersion; r.PayloadVersion
                     r.SessionID; r.ModuleID; r.SuiteID; r.TraceID; r.SpanID; r.ParentID
                     r.ITRCorrelationID; r.Service; r.Env; r.Name; r.Resource; r.SpanType
                     r.Start; r.DurationNs; r.Error
                     r.TestName; r.TestSuite; r.TestModule; r.TestFramework; r.TestFrameworkVersion
                     r.TestStatus; r.TestType; r.TestSourceFile; Col.opt r.TestSourceStart; Col.opt r.TestSourceEnd
                     r.TestParameters; r.TestCodeowners; r.TestCommand; r.TestSessionName
                     r.GitRepositoryURL; r.GitBranch; r.GitTag; r.GitCommitSHA; r.GitCommitMessage
                     r.GitCommitAuthorName; r.GitCommitAuthorEmail; r.GitCommitAuthorDate
                     r.GitCommitCommitterName; r.GitCommitCommitterEmail; r.GitCommitCommitterDate
                     r.CIProviderName; r.CIPipelineID; r.CIPipelineName; r.CIPipelineNumber; r.CIPipelineURL
                     r.CIJobID; r.CIJobName; r.CIJobURL; r.CIStageName; r.CIWorkspacePath
                     r.CINodeName; r.CINodeLabels
                     r.OSPlatform; r.OSVersion; r.OSArchitecture; r.RuntimeName; r.RuntimeVersion
                     r.Language; r.RuntimeID; r.LibraryVersion
                     Col.opt r.TestIsNew; Col.opt r.TestIsRetry; Col.opt r.TestIsModified; Col.opt r.TestSkippedByITR
                     Col.opt r.ITRUnskippable; Col.opt r.ITRForcedRun; Col.opt r.CodeCoverageEnabled
                     r.TestRetryReason; r.EarlyFlakeAbortReason
                     r.EVPSubdomain; r.ContainerID; r.AgentHostname; r.AgentVersion
                     r.Meta; r.Metrics; r.Metadata; r.Content |])
          with
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

module CICoverage =
    let table: Table<CICoverageRow> =
        { Table.create
              "storage_ci_coverage"
              "ci_coverage"
              [ "tenant_id"; "received_at"; "payload_version"; "session_id"; "suite_id"; "span_id"
                "files_filename"; "files_bitmap"; "raw"; "raw_format"; "event"; "extra"
                "evp_subdomain"; "container_id"; "agent_hostname"; "agent_version" ]
              (fun (r: CICoverageRow) ->
                  [| r.TenantID; r.ReceivedAt; r.PayloadVersion; r.SessionID; r.SuiteID; Col.opt r.SpanID
                     r.FilesFilename; r.FilesBitmap; r.Raw; r.RawFormat; r.Event; r.Extra
                     r.EVPSubdomain; r.ContainerID; r.AgentHostname; r.AgentVersion |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 10_000
              MaxInFlight = 1 }

module GitCommits =
    let table: Table<GitCommitRow> =
        { Table.create
              "storage_git_commits"
              "git_commits"
              [ "tenant_id"; "repository_url"; "sha"; "seen_at"; "packfile_id"; "source" ]
              (fun (r: GitCommitRow) -> [| r.TenantID; r.RepositoryURL; r.SHA; r.SeenAt; Col.opt r.PackfileID; r.Source |])
          with
              MaxRows = 5000
              FlushInterval = TimeSpan.FromSeconds 5.0 }

module GitPackfiles =
    let table: Table<GitPackfileRow> =
        { Table.create
              "storage_git_packfiles"
              "git_packfiles"
              [ "tenant_id"; "received_at"; "repository_url"; "pushed_sha"; "packfile_id"
                "filename"; "packfile"; "size_bytes"; "other_parts" ]
              (fun (r: GitPackfileRow) ->
                  [| r.TenantID; r.ReceivedAt; r.RepositoryURL; r.PushedSHA; r.PackfileID
                     r.Filename; r.Packfile; r.SizeBytes; r.OtherParts |])
          with
              MaxRows = 50
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 2_000
              MaxInFlight = 1 }

module CISettingsRequests =
    let table: Table<CISettingsRequestRow> =
        { Table.create
              "storage_ci_settings_requests"
              "ci_settings_requests"
              [ "tenant_id"; "received_at"; "endpoint"; "request_id"; "request_type"
                "service"; "env"; "repository_url"; "branch"; "sha"; "test_level"
                "module"; "commit_message"; "page_state"; "configurations"
                "request_body"; "response_body" ]
              (fun (r: CISettingsRequestRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Endpoint; r.RequestID; r.RequestType
                     r.Service; r.Env; r.RepositoryURL; r.Branch; r.SHA; r.TestLevel
                     r.Module; r.CommitMessage; r.PageState; r.Configurations
                     r.RequestBody; r.ResponseBody |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 5.0 }

module CIWebhookEvents =
    let table: Table<CIWebhookEventRow> =
        { Table.create
              "storage_ci_webhook_events"
              "ci_webhook_events"
              [ "tenant_id"; "received_at"; "provider"; "level"; "service"
                "payload_version"; "partial_retry"; "is_manual"
                "trace_id"; "span_id"; "parent_span_id"; "id"; "unique_id"
                "pipeline_id"; "pipeline_unique_id"; "pipeline_name"
                "stage_id"; "stage_name"; "parent_stage_id"
                "name"; "url"; "status"
                "start_raw"; "start_parsed"; "end_raw"; "end_parsed"; "queue_time_ms"
                "node_name"; "node_hostname"; "node_workspace"; "node_labels"
                "git_repository_url"; "git_default_branch"; "git_branch"; "git_sha"; "git_tag"
                "git_message"; "git_author_name"; "git_author_email"; "git_author_time"
                "git_committer_name"; "git_committer_email"; "git_commit_time"
                "user_name"; "user_email"
                "error_message"; "error_type"; "error_domain"; "error_stack"
                "parent_pipeline_trace_id"; "parent_pipeline_url"
                "parameters"; "tags"; "delivery_id"; "headers"; "extra"; "body" ]
              (fun (r: CIWebhookEventRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Provider; r.Level; r.Service
                     Col.opt r.PayloadVersion; Col.opt r.PartialRetry; Col.opt r.IsManual
                     r.TraceID; r.SpanID; r.ParentSpanID; r.ID; r.UniqueID
                     r.PipelineID; r.PipelineUniqueID; r.PipelineName
                     r.StageID; r.StageName; r.ParentStageID
                     r.Name; r.URL; r.Status
                     r.StartRaw; Col.opt r.StartParsed; r.EndRaw; Col.opt r.EndParsed; Col.opt r.QueueTimeMs
                     r.NodeName; r.NodeHostname; r.NodeWorkspace; r.NodeLabels
                     r.GitRepositoryURL; r.GitDefaultBranch; r.GitBranch; r.GitSHA; r.GitTag
                     r.GitMessage; r.GitAuthorName; r.GitAuthorEmail; r.GitAuthorTime
                     r.GitCommitterName; r.GitCommitterEmail; r.GitCommitTime
                     r.UserName; r.UserEmail
                     r.ErrorMessage; r.ErrorType; r.ErrorDomain; r.ErrorStack
                     r.ParentPipelineTraceID; r.ParentPipelineURL
                     r.Parameters; r.Tags; r.DeliveryID; r.Headers; r.Extra; r.Body |])
          with
              MaxRows = 1000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 20_000
              MaxInFlight = 2 }

module CIPipelineEvents =
    let table: Table<CIPipelineEventRow> =
        { Table.create
              "storage_ci_pipeline_events"
              "ci_pipeline_events"
              [ "tenant_id"; "received_at"; "kind"; "data_type"; "provider"; "ci_level"; "ci_env"
                "tags"; "metrics"; "span_name"; "span_start_raw"; "span_end_raw"
                "attributes"; "body"; "extra" ]
              (fun (r: CIPipelineEventRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Kind; r.DataType; r.Provider; Col.opt r.CILevel; r.CIEnv
                     r.Tags; r.Metrics; r.SpanName; r.SpanStartRaw; r.SpanEndRaw
                     r.Attributes; r.Body; r.Extra |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 2.0 }

module SyntheticsTestConfigs =
    let table: Table<SyntheticsTestConfigRow> =
        { Table.create
              "storage_synthetics_test_configs"
              "synthetics_test_configs"
              [ "tenant_id"; "updated_at"; "public_id"; "version"; "type"; "subtype"
                "tick_every"; "org_id"; "main_dc"; "result_id"; "run_type"
                "assertions"; "request"; "enabled" ]
              (fun (r: SyntheticsTestConfigRow) ->
                  [| r.TenantID; r.UpdatedAt; r.PublicID; r.Version; r.Type; r.Subtype
                     r.TickEvery; r.OrgID; r.MainDC; r.ResultID; r.RunType
                     r.Assertions; r.Request; r.Enabled |])
          with
              MaxRows = 100
              FlushInterval = TimeSpan.FromSeconds 10.0 }
