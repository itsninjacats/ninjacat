/// What one node sends another. Every such message implements
/// `IMonitorMessage`, the one type the serializer is bound to; strict
/// serialization refuses anything else, so a message without the marker fails
/// in the cluster tests rather than in production.
module NinjaCat.Api.Monitors.Messages

open System.Runtime.Serialization
open System.Text.Json
open System.Text.Json.Serialization
open Akka.Actor
open Akka.Serialization

/// Its own options, not the engine's: those follow Datadog's wire format, and
/// a change made there must not change what nodes of two versions say to
/// each other during a rollout.
let private json =
    let o = JsonSerializerOptions(JsonSerializerDefaults.Web)
    o.Converters.Add(JsonFSharpConverter())
    o

type IMonitorMessage =
    interface
    end

/// A version of the monitor exists; a newer one than the entity has is read.
type Ensure =
    { Monitor: string
      Version: int }

    interface IMonitorMessage

/// The monitor was deleted: its groups are closed and the entity stops.
type Delete =
    { Monitor: string
      Version: int }

    interface IMonitorMessage

/// Stop for a hand-off, once nothing is left to store.
type Stop =
    | Stop

    interface IMonitorMessage

/// The monitor messages as JSON, the manifest naming the type.
type MessageSerializer(system: ExtendedActorSystem) =
    inherit SerializerWithStringManifest(system)

    override _.Identifier = 7100

    override _.Manifest(message: obj) =
        match message with
        | :? Ensure -> "ensure"
        | :? Delete -> "delete"
        | :? Stop -> "stop"
        | _ -> raise (SerializationException $"Not a monitor message: {message.GetType()}")

    override _.ToBinary(message: obj) =
        JsonSerializer.SerializeToUtf8Bytes(message, message.GetType(), json)

    override _.FromBinary(bytes: byte[], manifest: string) =
        match manifest with
        | "ensure" -> box (JsonSerializer.Deserialize<Ensure>(bytes, json))
        | "delete" -> box (JsonSerializer.Deserialize<Delete>(bytes, json))
        | "stop" -> box (JsonSerializer.Deserialize<Stop>(bytes, json))
        | _ -> raise (SerializationException $"Not a monitor message manifest: {manifest}")
