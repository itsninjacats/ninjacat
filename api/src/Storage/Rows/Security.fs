namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One CWS activity dump request: the "event" header part, the envelope and
/// metadata of the "dump" part, and the dump itself.
type CWSActivityDumpRow =
    { TenantID: string
      ReceivedAt: DateTime
      DumpID: Guid
      HeaderHost: string
      HeaderService: string
      HeaderSource: string
      HeaderTags: Map<string, string[]>
      /// The header's dns_names as JSON text.
      DNSNames: string
      /// Header keys without a column, each as JSON text.
      HeaderExtra: Map<string, string>
      /// The "event" part as it arrived, whether or not it was JSON.
      HeaderRaw: byte[]
      DumpHost: string
      DumpService: string
      DumpSource: string
      DumpTags: Map<string, string[]>
      AgentVersion: string
      AgentCommit: string
      KernelVersion: string
      LinuxDistribution: string
      Arch: string
      MetadataName: string
      ProtobufVersion: string
      DifferentiateArgs: uint8
      Comm: string
      ContainerID: string
      /// None without metadata: a real dump can report 0 here (the counters
      /// are relative to kernel boot), so 0 cannot stand for "absent".
      Start: uint64 option
      End: uint64 option
      Size: uint64 option
      Serialization: string
      CgroupID: string
      CgroupManager: string
      /// Every node of the tree, at every depth.
      TreeNodeCount: uint32
      /// The "dump" part as it arrived: protobuf.
      Dump: byte[] }

/// One process node of a dump's tree, flattened depth-first.
type CWSDumpNodeRow =
    { TenantID: string
      ReceivedAt: DateTime
      DumpID: Guid
      /// The index path from the root; enough to rebuild the tree.
      NodePath: uint32[]
      Depth: uint16
      ParentPath: uint32[]
      /// The node as protobuf JSON, without its children.
      Node: string
      PID: uint32
      PPID: uint32
      Comm: string
      ContainerID: string
      FilePath: string
      Args: string[]
      ImageTags: string[]
      MatchedRuleIDs: string[]
      GenerationType: string
      FilesCount: uint32
      DNSCount: uint32
      SocketsCount: uint32
      SyscallsCount: uint32 }

/// One logs-pipeline envelope from secruntime, secinfo or compliance, with
/// the event inside its "message" when that decoded.
type SecurityEventRow =
    { TenantID: string
      ReceivedAt: DateTime
      Track: string
      SeqInBatch: uint32
      Timestamp: DateTime option
      Hostname: string
      Service: string
      DDSource: string
      Status: string
      DDTags: Map<string, string[]>
      /// Envelope keys without a column, each as JSON text.
      Extra: Map<string, string>
      /// The envelope's "message" as it arrived.
      MessageRaw: string
      /// The event inside "message" as JSON; "" when it did not decode.
      Message: string
      /// Tells "decoded to an empty object" from "did not decode".
      MessageDecoded: uint8
      RuleID: string
      PolicyName: string
      EvtName: string
      EvtCategory: string
      Title: string
      EventKind: string }

/// One entity of an SBOM payload. EntityID is minted on arrival, never taken
/// from the wire's own id, and is what components and vulnerabilities join on.
type SBOMEntityRow =
    { TenantID: string
      ReceivedAt: DateTime
      EntityID: Guid
      PayloadVersion: int32
      Host: string
      Source: string option
      DdEnv: string option
      Type: string
      ID: string
      GeneratedAt: DateTime option
      RepoTags: string[]
      RepoDigests: string[]
      InUse: uint8
      GenerationDurationMs: int64 option
      DDTags: Map<string, string[]>
      Heartbeat: uint8
      Hash: string
      Status: string
      KernelVersion: string
      CPUArchitecture: string
      /// Error and Bom are the two arms of the wire's oneof.
      Error: string
      Bom: string
      /// The BOM as base64 protobuf, only when it could not be written as JSON.
      BomRaw: string
      ComponentCount: uint32
      VulnerabilityCount: uint32 }

/// One CycloneDX component; sub-components get rows of their own, linked by
/// ParentBomRef.
type SBOMComponentRow =
    { TenantID: string
      ReceivedAt: DateTime
      EntityID: Guid
      BomRef: string
      ParentBomRef: string
      Depth: uint16
      Type: string
      Name: string
      Version: string
      /// None is "never set", which CycloneDX keeps apart from "".
      Purl: string option
      Cpe: string option
      Group: string option
      Publisher: string option
      Author: string option
      Description: string option
      Scope: string
      Licenses: string[]
      Hashes: Map<string, string>
      /// A multiset: CycloneDX allows properties to repeat a name.
      Properties: Map<string, string[]>
      /// Protobuf JSON arrays of the repeated fields.
      ExternalReferences: string
      Evidence: string }

/// One CycloneDX vulnerability of an entity's BOM.
type SBOMVulnerabilityRow =
    { TenantID: string
      ReceivedAt: DateTime
      EntityID: Guid
      BomRef: string
      ID: string
      SourceName: string option
      SourceURL: string option
      Ratings: string
      Cwes: int32[]
      Description: string option
      Detail: string option
      Recommendation: string option
      Advisories: string
      Created: DateTime option
      Published: DateTime option
      Updated: DateTime option
      AnalysisState: string
      AnalysisJustification: string
      AnalysisResponse: string[]
      AnalysisDetail: string
      AffectsRefs: string[]
      Affects: string
      Properties: Map<string, string[]> }

module CwsActivityDumps =
    let table: Table<CWSActivityDumpRow> =
        { Table.create
              "storage_cws_activity_dumps"
              "cws_activity_dumps"
              [ "tenant_id"; "received_at"; "dump_id"
                "header_host"; "header_service"; "header_source"; "header_tags"; "dns_names"; "header_extra"; "header_raw"
                "dump_host"; "dump_service"; "dump_source"; "dump_tags"
                "agent_version"; "agent_commit"; "kernel_version"; "linux_distribution"
                "arch"; "metadata_name"; "protobuf_version"; "differentiate_args"
                "comm"; "container_id"; "start_raw"; "end_raw"; "size_raw"
                "serialization"; "cgroup_id"; "cgroup_manager"
                "tree_node_count"; "dump" ]
              (fun (r: CWSActivityDumpRow) ->
                  [| r.TenantID; r.ReceivedAt; r.DumpID
                     r.HeaderHost; r.HeaderService; r.HeaderSource; r.HeaderTags; r.DNSNames; r.HeaderExtra; r.HeaderRaw
                     r.DumpHost; r.DumpService; r.DumpSource; r.DumpTags
                     r.AgentVersion; r.AgentCommit; r.KernelVersion; r.LinuxDistribution
                     r.Arch; r.MetadataName; r.ProtobufVersion; r.DifferentiateArgs
                     r.Comm; r.ContainerID; Col.opt r.Start; Col.opt r.End; Col.opt r.Size
                     r.Serialization; r.CgroupID; r.CgroupManager
                     r.TreeNodeCount; r.Dump |])
          with
              MaxRows = 100
              FlushInterval = TimeSpan.FromSeconds 10.0 }

module CwsDumpNodes =
    let table: Table<CWSDumpNodeRow> =
        { Table.create
              "storage_cws_dump_nodes"
              "cws_dump_nodes"
              [ "tenant_id"; "received_at"; "dump_id"; "node_path"; "depth"; "parent_path"; "node"
                "pid"; "ppid"; "comm"; "container_id"; "file_path"; "args"; "image_tags"
                "matched_rule_ids"; "generation_type"
                "files_count"; "dns_count"; "sockets_count"; "syscalls_count" ]
              (fun (r: CWSDumpNodeRow) ->
                  [| r.TenantID; r.ReceivedAt; r.DumpID; r.NodePath; r.Depth; r.ParentPath; r.Node
                     r.PID; r.PPID; r.Comm; r.ContainerID; r.FilePath; r.Args; r.ImageTags
                     r.MatchedRuleIDs; r.GenerationType
                     r.FilesCount; r.DNSCount; r.SocketsCount; r.SyscallsCount |])
          with
              // One dump unpacks into thousands of nodes in a single request.
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

module SecurityEvents =
    let table: Table<SecurityEventRow> =
        { Table.create
              "storage_security_events"
              "security_events"
              [ "tenant_id"; "received_at"; "track"; "seq_in_batch"; "timestamp"
                "hostname"; "service"; "ddsource"; "status"; "ddtags"; "extra"
                "message_raw"; "message"; "message_decoded"
                "rule_id"; "policy_name"; "evt_name"; "evt_category"; "title"; "event_kind" ]
              (fun (r: SecurityEventRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Track; r.SeqInBatch; Col.opt r.Timestamp
                     r.Hostname; r.Service; r.DDSource; r.Status; r.DDTags; r.Extra
                     r.MessageRaw; r.Message; r.MessageDecoded
                     r.RuleID; r.PolicyName; r.EvtName; r.EvtCategory; r.Title; r.EventKind |])
          with
              // Logs-shaped traffic, so the logs table's numbers.
              MaxRows = 20000
              FlushInterval = TimeSpan.FromSeconds 1.0
              BufferLimit = 500_000 }

module SbomEntities =
    let table: Table<SBOMEntityRow> =
        { Table.create
              "storage_sbom_entities"
              "sbom_entities"
              [ "tenant_id"; "received_at"; "entity_id"; "payload_version"; "host"; "source"; "dd_env"
                "type"; "id"; "generated_at"; "repo_tags"; "repo_digests"; "in_use"
                "generation_duration_ms"; "dd_tags"; "heartbeat"; "hash"; "status"
                "kernel_version"; "cpu_architecture"; "error"; "bom"; "bom_raw"
                "component_count"; "vulnerability_count" ]
              (fun (r: SBOMEntityRow) ->
                  [| r.TenantID; r.ReceivedAt; r.EntityID; r.PayloadVersion; r.Host; Col.opt r.Source; Col.opt r.DdEnv
                     r.Type; r.ID; Col.opt r.GeneratedAt; r.RepoTags; r.RepoDigests; r.InUse
                     Col.opt r.GenerationDurationMs; r.DDTags; r.Heartbeat; r.Hash; r.Status
                     r.KernelVersion; r.CPUArchitecture; r.Error; r.Bom; r.BomRaw
                     r.ComponentCount; r.VulnerabilityCount |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 20_000
              MaxInFlight = 2 }

module SbomComponents =
    let table: Table<SBOMComponentRow> =
        { Table.create
              "storage_sbom_components"
              "sbom_components"
              // `group` is a keyword in ClickHouse, hence the backticks.
              [ "tenant_id"; "received_at"; "entity_id"; "bom_ref"; "parent_bom_ref"; "depth"
                "type"; "name"; "version"; "purl"; "cpe"; "`group`"; "publisher"; "author"
                "description"; "scope"; "licenses"; "hashes"; "properties"
                "external_references"; "evidence" ]
              (fun (r: SBOMComponentRow) ->
                  [| r.TenantID; r.ReceivedAt; r.EntityID; r.BomRef; r.ParentBomRef; r.Depth
                     r.Type; r.Name; r.Version; Col.opt r.Purl; Col.opt r.Cpe; Col.opt r.Group; Col.opt r.Publisher; Col.opt r.Author
                     Col.opt r.Description; r.Scope; r.Licenses; r.Hashes; r.Properties
                     r.ExternalReferences; r.Evidence |])
          with
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

module SbomVulnerabilities =
    let table: Table<SBOMVulnerabilityRow> =
        { Table.create
              "storage_sbom_vulnerabilities"
              "sbom_vulnerabilities"
              [ "tenant_id"; "received_at"; "entity_id"; "bom_ref"; "id"; "source_name"; "source_url"
                "ratings"; "cwes"; "description"; "detail"; "recommendation"; "advisories"
                "created"; "published"; "updated"
                "analysis_state"; "analysis_justification"; "analysis_response"; "analysis_detail"
                "affects_refs"; "affects"; "properties" ]
              (fun (r: SBOMVulnerabilityRow) ->
                  [| r.TenantID; r.ReceivedAt; r.EntityID; r.BomRef; r.ID; Col.opt r.SourceName; Col.opt r.SourceURL
                     r.Ratings; r.Cwes; Col.opt r.Description; Col.opt r.Detail; Col.opt r.Recommendation; r.Advisories
                     Col.opt r.Created; Col.opt r.Published; Col.opt r.Updated
                     r.AnalysisState; r.AnalysisJustification; r.AnalysisResponse; r.AnalysisDetail
                     r.AffectsRefs; r.Affects; r.Properties |])
          with
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 50_000
              MaxInFlight = 2 }
