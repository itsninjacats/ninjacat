namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One Database Monitoring event. All six tracks and every database engine
/// share the table: the engine-specific row arrays (oracle_rows,
/// postgres_activity, …) have no published schema, so the fields every track
/// agrees on are columns and the complete event is kept beside them.
type DBMEventRow =
    { TenantID: string
      ReceivedAt: DateTime
      /// The route the event arrived on: dbmmetrics, dbmactivity,
      /// databasequery, dbmmetadata, dbmhealth or dbmcolumnstatistics.
      Track: string
      /// The event's own timestamp; None when it sent none, never 1970.
      Timestamp: DateTime option
      /// The database host, not the agent's.
      Host: string
      DatabaseInstance: string
      AgentHostname: string
      AgentVersion: string
      Source: string
      DBMType: string
      Kind: string
      DBMS: string
      DBMSVersion: string
      /// None when not reported: 0 is a value an integration could send.
      CollectionInterval: float option
      Tags: Map<string, string[]>
      /// The keys with no column, sorted: the engine's row arrays live here.
      ExtraKeys: string[]
      /// Envelope keys whose value had an unexpected shape.
      UndecodedKeys: string[]
      /// The complete event exactly as received.
      Event: string
      /// From the "db" object of a databasequery event; None elsewhere.
      DBInstance: string option
      QuerySignature: string option
      Statement: string option
      PlanSignature: string option
      PlanDefinitionSteps: uint32 option }

module DbmEvents =
    let table: Table<DBMEventRow> =
        { Table.create
              "storage_dbm_events"
              "dbm_events"
              [ "tenant_id"; "received_at"; "track"; "timestamp"
                "host"; "database_instance"; "agent_hostname"; "agent_version"
                "source"; "dbm_type"; "kind"; "dbms"; "dbms_version"
                "collection_interval"; "tags"
                "extra_keys"; "undecoded_keys"
                "event"
                "db_instance"; "query_signature"; "statement"; "plan_signature"; "plan_definition_steps" ]
              (fun (r: DBMEventRow) ->
                  [| r.TenantID; r.ReceivedAt; r.Track; Col.opt r.Timestamp
                     r.Host; r.DatabaseInstance; r.AgentHostname; r.AgentVersion
                     r.Source; r.DBMType; r.Kind; r.DBMS; r.DBMSVersion
                     Col.opt r.CollectionInterval; r.Tags
                     r.ExtraKeys; r.UndecodedKeys
                     r.Event
                     Col.opt r.DBInstance; Col.opt r.QuerySignature; Col.opt r.Statement; Col.opt r.PlanSignature
                     Col.opt r.PlanDefinitionSteps |])
          with
              // A row carries a whole event, which can hold hundreds of sessions.
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 20_000
              MaxInFlight = 2 }
