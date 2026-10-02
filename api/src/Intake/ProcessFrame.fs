/// The envelope of the process-agent and of the orchestrator collector: a
/// small binary header that says which protobuf message follows and how it
/// is encoded, then the message.
///
/// Ported from github.com/DataDog/agent-payload, process/message.go.
namespace NinjaCat.Api.Intake

open System
open System.Buffers.Binary
open System.IO
open System.Text
open Google.Protobuf
open Datadog.ProcessAgent

type FrameHeader =
    { Version: uint8
      /// 0 protobuf, 1 JSON, 2 zstd 0.x (not supported), 4 and 5 zstd 1.x.
      Encoding: uint8
      Type: uint8
      SubscriptionID: uint8
      OrgID: int32
      Timestamp: int64 }

type Frame = { Header: FrameHeader; Body: IMessage }

module ProcessFrame =
    /// Message type → the parser of its protobuf message.
    let private parsers: Map<uint8, MessageParser> =
        Map
            [ 12uy, CollectorProc.Parser :> MessageParser
              22uy, CollectorConnections.Parser
              23uy, ResCollector.Parser
              27uy, CollectorRealTime.Parser
              39uy, CollectorContainer.Parser
              40uy, CollectorContainerRealTime.Parser
              41uy, CollectorPod.Parser
              42uy, CollectorReplicaSet.Parser
              43uy, CollectorDeployment.Parser
              44uy, CollectorService.Parser
              45uy, CollectorNode.Parser
              46uy, CollectorCluster.Parser
              47uy, CollectorJob.Parser
              48uy, CollectorCronJob.Parser
              49uy, CollectorDaemonSet.Parser
              50uy, CollectorStatefulSet.Parser
              51uy, CollectorPersistentVolume.Parser
              52uy, CollectorPersistentVolumeClaim.Parser
              53uy, CollectorProcDiscovery.Parser
              54uy, CollectorRole.Parser
              55uy, CollectorRoleBinding.Parser
              56uy, CollectorClusterRole.Parser
              57uy, CollectorClusterRoleBinding.Parser
              58uy, CollectorServiceAccount.Parser
              59uy, CollectorIngress.Parser
              60uy, CollectorProcEvent.Parser
              61uy, CollectorNamespace.Parser
              80uy, CollectorManifest.Parser
              81uy, CollectorManifestCRD.Parser
              82uy, CollectorManifestCR.Parser
              83uy, CollectorVerticalPodAutoscaler.Parser
              84uy, CollectorHorizontalPodAutoscaler.Parser
              85uy, CollectorNetworkPolicy.Parser
              86uy, CollectorLimitRange.Parser
              87uy, CollectorStorageClass.Parser
              88uy, CollectorPodDisruptionBudget.Parser
              200uy, CollectorECSTask.Parser ]

    /// The header and where the message starts. Three versions exist; each
    /// later one appends fields: v1 is 4 bytes, v2 adds the org id (8), v3
    /// adds the timestamp (16).
    let readHeader (data: byte[]) : Result<FrameHeader * int, string> =
        if data.Length <= 4 then
            Error $"invalid message length: {data.Length}"
        else
            let header =
                { Version = data[0]
                  Encoding = data[1]
                  Type = data[2]
                  SubscriptionID = data[3]
                  OrgID = 0
                  Timestamp = 0L }

            match data[0] with
            | 1uy -> Ok(header, 4)
            | 2uy when data.Length >= 8 ->
                Ok({ header with OrgID = BinaryPrimitives.ReadInt32LittleEndian(ReadOnlySpan(data, 4, 4)) }, 8)
            | 3uy when data.Length >= 16 ->
                Ok(
                    { header with
                        OrgID = BinaryPrimitives.ReadInt32LittleEndian(ReadOnlySpan(data, 4, 4))
                        Timestamp = BinaryPrimitives.ReadInt64LittleEndian(ReadOnlySpan(data, 8, 8)) },
                    16
                )
            | 2uy
            | 3uy -> Error "unexpected EOF"
            | version -> Error $"invalid message version: {version}"

    let private unzstd (body: byte[]) : byte[] =
        use input = new MemoryStream(body)
        use decoder = new ZstdSharp.DecompressionStream(input)
        Body.readInflated decoder

    /// The whole frame: header and decoded message.
    let decode (data: byte[]) : Result<Frame, string> =
        match readHeader data with
        | Error e -> Error e
        | Ok(header, offset) ->
            match parsers.TryFind header.Type with
            | None -> Error $"unhandled message type: {header.Type}"
            | Some parser ->
                let body = data[offset..]

                try
                    match header.Encoding with
                    | 0uy -> Ok { Header = header; Body = parser.ParseFrom body }
                    | 1uy ->
                        let descriptor = parser.ParseFrom(Array.empty<byte>).Descriptor
                        Ok { Header = header; Body = JsonParser.Default.Parse(Encoding.UTF8.GetString body, descriptor) }
                    // Zstandard 0.x predates the format every current library
                    // reads. Agents have sent encoding 4 or 5 for years. The
                    // caller keeps such a frame in raw_payloads.
                    | 2uy -> Error "zstd 0.x frames (message encoding 2) are not supported"
                    | 4uy
                    | 5uy -> Ok { Header = header; Body = parser.ParseFrom(unzstd body) }
                    | other -> Error $"unknown message encoding: {other}"
                with e ->
                    Error e.Message

    /// A frame as the intake answers with one: header v3, plain protobuf.
    let encode (messageType: uint8) (timestamp: int64) (body: IMessage) : byte[] =
        let payload = body.ToByteArray()
        let frame = Array.zeroCreate<byte> (16 + payload.Length)
        frame[0] <- 3uy
        frame[1] <- 0uy
        frame[2] <- messageType
        BinaryPrimitives.WriteInt64LittleEndian(Span(frame, 8, 8), timestamp)
        Array.blit payload 0 frame 16 payload.Length
        frame
