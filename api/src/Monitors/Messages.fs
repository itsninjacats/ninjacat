/// What nodes send each other; every message implements IMonitorMessage, the one type the serializer is bound to.
module NinjaCat.Api.Monitors.Messages

open System.Runtime.Serialization
open System.Text.Json
open System.Text.Json.Serialization
open Akka.Actor
open Akka.Serialization

/// Own options, so a change to the engine's cannot break a rolling upgrade.
let private json =
    let o = JsonSerializerOptions(JsonSerializerDefaults.Web)
    o.Converters.Add(JsonFSharpConverter())
    o

type IMonitorMessage =
    interface
    end

type Ensure =
    { Monitor: string
      Version: int }

    interface IMonitorMessage

type Delete =
    { Monitor: string
      Version: int }

    interface IMonitorMessage

/// The hand-off stop.
type Stop =
    | Stop

    interface IMonitorMessage

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
