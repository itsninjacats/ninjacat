namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One multipart submission to intake.profile.<site>.
///
/// Samples are not exploded into rows: a CPU profile carries tens of
/// thousands of them, and pprof reads the bytes kept in AttachBytes. What is
/// kept per attachment is the pprof envelope, as parallel arrays, so a query
/// can filter profiles by shape without parsing them again.
type ProfileRow =
    { TenantID: string
      ReceivedAt: DateTime
      /// Which path produced the row: "profile" or "profile-v1".
      Variant: string
      DDEvpOrigin: string
      DDEvpOriginVersion: string
      /// The "event" part as it arrived. The fields after it are read from it.
      Event: byte[]
      StartRaw: string
      StartParsed: DateTime option
      EndRaw: string
      EndParsed: DateTime option
      Family: string
      Version: string
      Runtime: string
      Language: string
      TagsProfiler: Map<string, string[]>
      /// One entry per part other than "event", ordered by part name: the
      /// order on the wire is not kept.
      AttachName: string[]
      /// The part as it arrived, still gzip-compressed if it was.
      AttachBytes: byte[][]
      AttachSize: uint64[]
      /// 1 when the part is pprof; the fields after it are zero when it is not.
      AttachParsed: uint8[]
      AttachSampleTypes: string[][]
      AttachSampleUnits: string[][]
      AttachSampleCount: uint64[]
      AttachTimeNanos: int64[]
      AttachDurationNanos: int64[]
      AttachPeriodType: string[]
      AttachPeriod: int64[]
      AttachMappingCount: uint32[]
      AttachLocationCount: uint32[]
      AttachFunctionCount: uint32[] }

/// One entry of a Dynamic Instrumentation logs batch.
type DebuggerLogRow =
    { TenantID: string
      ReceivedAt: DateTime
      Service: string
      DDSource: string
      /// The `ddtags` query parameter: it describes the whole batch.
      DDTags: Map<string, string[]>
      /// The array element or NDJSON line as it arrived.
      Entry: byte[]
      /// The entry's top-level keys other than service and ddsource.
      ExtraKeys: string[] }

/// One probe status message.
type DebuggerDiagnosticRow =
    { TenantID: string
      ReceivedAt: DateTime
      /// The message's own time; None when missing or unreadable, never the
      /// receive time.
      Timestamp: DateTime option
      Service: string
      DDSource: string
      DDTags: Map<string, string[]>
      RuntimeID: string
      ProbeID: string
      Status: string
      ProbeVersion: string
      /// Both None without an exception, so "no exception" and "an exception
      /// with an empty message" stay apart.
      ExceptionType: string option
      ExceptionMessage: string option
      /// The array element as it arrived.
      Message: byte[] }

/// One symbol-database upload.
///
/// The event part and the envelope inside the gzip file describe the same
/// upload but come from two serializers: the event says `uploadId`, the
/// envelope `upload_id`. Both are kept (the envelope's under Env…) so a
/// drift between them can be seen.
type SymdbUploadRow =
    { TenantID: string
      ReceivedAt: DateTime
      /// The "event" part as it arrived.
      Event: byte[]
      DDTags: Map<string, string[]>
      Service: string
      Version: string
      Language: string
      RuntimeID: string
      /// Identifiers, kept as the text on the wire: nothing says they stay numeric.
      UploadID: string
      BatchNum: string
      /// None when the event does not carry them, which is not "false" or "0".
      Final: uint8 option
      AttachmentSize: uint64 option
      /// The "file" part as it arrived, gzip-compressed.
      File: byte[]
      InflatedSize: uint64
      /// Top-level scopes only.
      ScopeCount: uint32
      /// 1 when the envelope's "scopes" was present and decoded.
      ScopesOK: uint8
      EnvService: string
      EnvVersion: string
      EnvLanguage: string
      EnvUploadID: string
      EnvBatchNum: string
      EnvFinal: string }

/// One native symbol upload from the host profiler.
type SymbolUploadRow =
    { TenantID: string
      ReceivedAt: DateTime
      Type: string
      Arch: string
      GNUBuildID: string
      GoBuildID: string
      FileHash: string
      SymbolSource: string
      Origin: string
      OriginVersion: string
      Filename: string
      /// The "event" part as it arrived.
      Meta: byte[]
      HasELF: uint8
      /// "ELF32", "ELF64", "ELF?"; "" without an ELF file.
      ELFClass: string
      /// "LE", "BE", "?"; "" without an ELF file.
      ELFEndianness: string
      ELF: byte[]
      ELFSize: uint64
      /// Every part other than "event" and "elf_symbol_file".
      OtherParts: Map<string, byte[]> }

module Profiles =
    let table: Table<ProfileRow> =
        { Table.create
              "storage_profiles"
              "profiles"
              [ "tenant_id"; "received_at"; "variant"; "dd_evp_origin"; "dd_evp_origin_version"
                "event"; "start_raw"; "start_parsed"; "end_raw"; "end_parsed"
                "family"; "version"; "runtime"; "language"; "tags_profiler"
                "attach_name"; "attach_bytes"; "attach_size"; "attach_parsed"
                "attach_sample_types"; "attach_sample_units"; "attach_sample_count"
                "attach_time_nanos"; "attach_duration_nanos"; "attach_period_type"; "attach_period"
                "attach_mapping_count"; "attach_location_count"; "attach_function_count" ]
              (fun (r: ProfileRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Variant; r.DDEvpOrigin; r.DDEvpOriginVersion
                     r.Event; r.StartRaw; Col.opt r.StartParsed; r.EndRaw; Col.opt r.EndParsed
                     r.Family; r.Version; r.Runtime; r.Language; r.TagsProfiler
                     r.AttachName; r.AttachBytes; r.AttachSize; r.AttachParsed
                     r.AttachSampleTypes; r.AttachSampleUnits; r.AttachSampleCount
                     r.AttachTimeNanos; r.AttachDurationNanos; r.AttachPeriodType; r.AttachPeriod
                     r.AttachMappingCount; r.AttachLocationCount; r.AttachFunctionCount |])
          with
              // A row is a whole submission with megabytes of pprof: few rows
              // per batch, one flush at a time.
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }

module DebuggerLogs =
    let table: Table<DebuggerLogRow> =
        { Table.create
              "storage_debugger_logs"
              "debugger_logs"
              [ "tenant_id"; "received_at"; "service"; "ddsource"; "ddtags"; "entry"; "extra_keys" ]
              (fun (r: DebuggerLogRow) -> [| r.TenantID; r.ReceivedAt; r.Service; r.DDSource; r.DDTags; r.Entry; r.ExtraKeys |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }

module DebuggerDiagnostics =
    let table: Table<DebuggerDiagnosticRow> =
        { Table.create
              "storage_debugger_diagnostics"
              "debugger_diagnostics"
              [ "tenant_id"; "received_at"; "timestamp"; "service"; "ddsource"; "ddtags"; "runtime_id"
                "probe_id"; "status"; "probe_version"; "exception_type"; "exception_message"; "message" ]
              (fun (r: DebuggerDiagnosticRow) ->
                  [| r.TenantID; r.ReceivedAt; Col.opt r.Timestamp; r.Service; r.DDSource; r.DDTags; r.RuntimeID
                     r.ProbeID; r.Status; r.ProbeVersion; Col.opt r.ExceptionType; Col.opt r.ExceptionMessage; r.Message |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 2.0 }

module SymdbUploads =
    let table: Table<SymdbUploadRow> =
        { Table.create
              "storage_symdb_uploads"
              "symdb_uploads"
              [ "tenant_id"; "received_at"; "event"; "ddtags"; "service"; "version"; "language"; "runtime_id"
                "upload_id"; "batch_num"; "final"; "attachment_size"
                "file"; "inflated_size"; "scope_count"; "scopes_ok"
                "env_service"; "env_version"; "env_language"; "env_upload_id"; "env_batch_num"; "env_final" ]
              (fun (r: SymdbUploadRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Event; r.DDTags; r.Service; r.Version; r.Language; r.RuntimeID
                     r.UploadID; r.BatchNum; Col.opt r.Final; Col.opt r.AttachmentSize
                     r.File; r.InflatedSize; r.ScopeCount; r.ScopesOK
                     r.EnvService; r.EnvVersion; r.EnvLanguage; r.EnvUploadID; r.EnvBatchNum; r.EnvFinal |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }

module SymbolUploads =
    let table: Table<SymbolUploadRow> =
        { Table.create
              "storage_symbol_uploads"
              "symbol_uploads"
              [ "tenant_id"; "received_at"; "type"; "arch"; "gnu_build_id"; "go_build_id"; "file_hash"
                "symbol_source"; "origin"; "origin_version"; "filename"; "meta"
                "has_elf"; "elf_class"; "elf_endianness"; "elf"; "elf_size"; "other_parts" ]
              (fun (r: SymbolUploadRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Type; r.Arch; r.GNUBuildID; r.GoBuildID; r.FileHash
                     r.SymbolSource; r.Origin; r.OriginVersion; r.Filename; r.Meta
                     r.HasELF; r.ELFClass; r.ELFEndianness; r.ELF; r.ELFSize; r.OtherParts |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }
