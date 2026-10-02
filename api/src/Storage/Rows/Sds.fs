/// Sensitive Data Scanner results: one payload is one scan of one resource,
/// with a result per location scanned and a match per rule that hit.
/// Reasoning per column: 0020_sds.sql.
namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// The payload itself: who scanned what, with which rules.
type SdsScanRow =
    { TenantID: string
      ReceivedAt: DateTime
      ScanID: Guid
      /// The scanner's clock; None when it sent 0.
      Timestamp: DateTime option
      ResourceType: string
      ResourceName: string
      /// "agent", "agentless", "datadog_crawler", or "" when not sent.
      ScanningSource: string
      SourceVersion: string
      SourceRegion: string
      SourceHostname: string
      SourceServiceName: string
      ScannerVersion: string
      ScannerRegion: string
      RuleIDs: string[]
      /// The deprecated rules map, as JSON; "" when empty.
      Rules: string
      /// None when the payload carried no statistics.
      ScanDurationMs: int64 option
      TotalFilesFound: int64 option
      FilesScanned: int64 option
      FilesSkippedUnsupportedType: int64 option
      FilesPartiallyScannedSizeLimit: int64 option
      TotalDataScannedBytes: int64 option
      SkippedFilesByType: Map<string, int64>
      ResultCount: uint32 }

module SdsScans =
    let table: Table<SdsScanRow> =
        Table.create
            "storage_sds_scans"
            "sds_scans"
            [ "tenant_id"; "received_at"; "scan_id"; "timestamp"; "resource_type"; "resource_name"
              "scanning_source"; "source_version"; "source_region"; "source_hostname"; "source_service_name"
              "scanner_version"; "scanner_region"; "rule_ids"; "rules"
              "scan_duration_ms"; "total_files_found"; "files_scanned"; "files_skipped_unsupported_type"
              "files_partially_scanned_size_limit"; "total_data_scanned_bytes"; "skipped_files_by_type"
              "result_count" ]
            (fun (r: SdsScanRow) ->
                [| r.TenantID; r.ReceivedAt; r.ScanID; Col.opt r.Timestamp; r.ResourceType; r.ResourceName
                   r.ScanningSource; r.SourceVersion; r.SourceRegion; r.SourceHostname; r.SourceServiceName
                   r.ScannerVersion; r.ScannerRegion; r.RuleIDs; r.Rules
                   Col.opt r.ScanDurationMs; Col.opt r.TotalFilesFound; Col.opt r.FilesScanned
                   Col.opt r.FilesSkippedUnsupportedType; Col.opt r.FilesPartiallyScannedSizeLimit
                   Col.opt r.TotalDataScannedBytes; r.SkippedFilesByType
                   r.ResultCount |])

/// Where a result was found: copied onto the result and onto its matches.
type SdsLocation =
    { /// "postgres_table", "rds_table", "snowflake_table", "s3_file", or
      /// "path" / "database" from the deprecated fields; "" when not sent.
      Kind: string
      DatabaseName: string
      SchemaName: string
      TableName: string
      Path: string }

module SdsLocation =
    let columns = [ "location_kind"; "database_name"; "schema_name"; "table_name"; "path" ]

    let values (l: SdsLocation) : obj list =
        [ l.Kind; l.DatabaseName; l.SchemaName; l.TableName; l.Path ]

/// One location that was scanned, with or without matches.
type SdsResultRow =
    { TenantID: string
      ReceivedAt: DateTime
      ScanID: Guid
      ResultIndex: uint32
      ResourceType: string
      ResourceName: string
      Location: SdsLocation
      TableRowCount: int64 option
      ScannedRowCount: int64 option
      ScannedColumnNames: string[]
      ScannedColumnTypes: string[]
      /// The whole location as protobuf's JSON: each kind has fields of its
      /// own that have no column.
      LocationJson: string
      Duration: int64
      TaskID: string
      SubTaskID: string
      TaskStartedAt: DateTime option
      TaskEndedAt: DateTime option
      /// The enum's name; "" when the result carried no task metadata.
      TaskStatus: string
      FailureReason: string
      MatchCount: uint32
      TableMatchCount: uint32 }

module SdsResults =
    let table: Table<SdsResultRow> =
        Table.create
            "storage_sds_results"
            "sds_results"
            ([ "tenant_id"; "received_at"; "scan_id"; "result_index"; "resource_type"; "resource_name" ]
             @ SdsLocation.columns
             @ [ "table_row_count"; "scanned_row_count"; "scanned_column_names"; "scanned_column_types"; "location"
                 "duration"; "task_id"; "sub_task_id"; "task_started_at"; "task_ended_at"; "task_status"
                 "failure_reason"; "match_count"; "table_match_count" ])
            (fun (r: SdsResultRow) ->
                let head: obj list = [ r.TenantID; r.ReceivedAt; r.ScanID; r.ResultIndex; r.ResourceType; r.ResourceName ]

                let tail: obj list =
                    [ Col.opt r.TableRowCount; Col.opt r.ScannedRowCount; r.ScannedColumnNames; r.ScannedColumnTypes
                      r.LocationJson; r.Duration; r.TaskID; r.SubTaskID; Col.opt r.TaskStartedAt; Col.opt r.TaskEndedAt
                      r.TaskStatus; r.FailureReason; r.MatchCount; r.TableMatchCount ]

                Array.ofList (head @ SdsLocation.values r.Location @ tail))

/// One rule that matched in one location: a match in text (a file), or a
/// count of matching rows in a column (a table).
type SdsMatchRow =
    { TenantID: string
      ReceivedAt: DateTime
      ScanID: Guid
      ResultIndex: uint32
      ResourceType: string
      ResourceName: string
      Location: SdsLocation
      /// "match" or "table_match".
      Kind: string
      RuleID: string
      // A match in text.
      Sample: string
      StartIndex: int64
      EndIndex: int64
      MatchPath: string option
      Line: int64 option
      Column: int64 option
      StartLine: int64 option
      EndLine: int64 option
      Row: int64 option
      StartIndexInLine: int64 option
      EndIndexInLine: int64 option
      MatchStatus: string option
      // A match in a table.
      ColumnName: string
      CountMatchedRows: int64
      CountTotalRows: int64
      CountMatches: int64 }

module SdsMatches =
    let table: Table<SdsMatchRow> =
        Table.create
            "storage_sds_matches"
            "sds_matches"
            ([ "tenant_id"; "received_at"; "scan_id"; "result_index"; "resource_type"; "resource_name" ]
             @ SdsLocation.columns
             @ [ "kind"; "rule_id"; "sample"; "start_index"; "end_index"; "match_path"; "line"; "column"
                 "start_line"; "end_line"; "row"; "start_index_in_line"; "end_index_in_line"; "match_status"
                 "column_name"; "count_matched_rows"; "count_total_rows"; "count_matches" ])
            (fun (r: SdsMatchRow) ->
                let head: obj list = [ r.TenantID; r.ReceivedAt; r.ScanID; r.ResultIndex; r.ResourceType; r.ResourceName ]

                let tail: obj list =
                    [ r.Kind; r.RuleID; r.Sample; r.StartIndex; r.EndIndex; Col.opt r.MatchPath; Col.opt r.Line
                      Col.opt r.Column; Col.opt r.StartLine; Col.opt r.EndLine; Col.opt r.Row
                      Col.opt r.StartIndexInLine; Col.opt r.EndIndexInLine; Col.opt r.MatchStatus
                      r.ColumnName; r.CountMatchedRows; r.CountTotalRows; r.CountMatches ]

                Array.ofList (head @ SdsLocation.values r.Location @ tail))
