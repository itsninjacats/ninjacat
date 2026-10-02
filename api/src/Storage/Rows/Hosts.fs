namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// Host metadata from /intake/. The table is a ReplacingMergeTree on
/// (tenant_id, host): repeated writes collapse to the newest.
///
/// The sections whose shape moves between agent versions are kept as their
/// JSON text. An empty string there means the agent did not send the
/// section, which is not the same statement as "{}".
type HostRow =
    { TenantID: string
      Host: string
      SeenAt: DateTime
      AgentVersion: string
      OS: string
      Platform: Map<string, string>
      CPU: Map<string, string>
      Memory: Map<string, string>
      /// The "system" tag source alone: the column charts filter on.
      Tags: Map<string, string[]>
      UUID: string
      AgentFlavor: string
      PythonVersion: string
      Meta: string
      Network: string
      Filesystem: string
      Logs: string
      OTLP: string
      SystemStats: Map<string, string>
      InstallMethod: Map<string, string>
      ProxyInfo: Map<string, string>
      ContainerMeta: Map<string, string>
      /// None when the agent sent no such key: "FIPS is off" and "this agent
      /// does not know about FIPS" are different statements.
      FIPSMode: uint8 option
      FIPSProxyEnabled: uint8 option
      /// Every tag source (system, google_tags, …) with its whole tag list.
      HostTags: Map<string, string[]>
      /// gohai sections without a column, and envelope keys the decoder does
      /// not know. Values JSON-encoded.
      GohaiExtra: Map<string, string>
      IntakeExtra: Map<string, string>
      /// The legacy V5 process snapshot riding on the host payload, verbatim.
      Resources: string }

module Hosts =
    let table: Table<HostRow> =
        { Table.create
              "storage_hosts"
              "hosts"
              [ "tenant_id"; "host"; "seen_at"; "agent_version"; "os"; "platform"; "cpu"; "memory"; "tags"
                "uuid"; "agent_flavor"; "python_version"; "meta"; "system_stats"; "network"; "filesystem"
                "logs"; "install_method"; "proxy_info"; "otlp"; "container_meta"
                "fips_mode"; "fips_proxy_enabled"; "host_tags"; "gohai_extra"; "intake_extra"; "resources" ]
              (fun r ->
                  [| r.TenantID; r.Host; r.SeenAt; r.AgentVersion; r.OS; r.Platform; r.CPU; r.Memory; r.Tags
                     r.UUID; r.AgentFlavor; r.PythonVersion; r.Meta; r.SystemStats; r.Network; r.Filesystem
                     r.Logs; r.InstallMethod; r.ProxyInfo; r.OTLP; r.ContainerMeta
                     Col.opt r.FIPSMode; Col.opt r.FIPSProxyEnabled; r.HostTags; r.GohaiExtra; r.IntakeExtra; r.Resources |])
          with
              MaxRows = 100
              FlushInterval = TimeSpan.FromSeconds 10.0 }
