/// Frames made by the agent's own encoder (agent-payload v5.0.207), one per
/// encoding, each carrying the same CollectorProc.
module NinjaCat.Api.Intake.Tests.ProcessFrameTests

open System
open System.IO
open Xunit
open Datadog.ProcessAgent
open NinjaCat.Api.Intake

let private frame (encoding: int) : byte[] =
    File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "Fixtures", "frames", $"frame_enc{encoding}.bin"))

let private assertTheProcess (data: byte[]) (encoding: uint8) =
    match ProcessFrame.decode data with
    | Error e -> Assert.Fail e
    | Ok decoded ->
        Assert.Equal(3uy, decoded.Header.Version)
        Assert.Equal(encoding, decoded.Header.Encoding)
        Assert.Equal(12uy, decoded.Header.Type)
        Assert.Equal(1790151330L, decoded.Header.Timestamp)
        let proc = decoded.Body :?> CollectorProc
        Assert.Equal("host-a", proc.HostName)
        Assert.Equal("vpc-1", proc.NetworkId)
        Assert.Equal(7, proc.GroupId)
        Assert.Equal(42, (Assert.Single proc.Processes).Pid)
        Assert.Equal<string list>([ "nginx"; "-g" ], List.ofSeq proc.Processes[0].Command.Args)

[<Fact>]
let ``plain protobuf`` () = assertTheProcess (frame 0) 0uy

[<Fact>]
let ``JSON`` () = assertTheProcess (frame 1) 1uy

[<Fact>]
let ``zstd 1.x, from the agent's cgo build`` () = assertTheProcess (frame 4) 4uy

[<Fact>]
let ``zstd from the agent's pure Go build`` () = assertTheProcess (frame 5) 5uy

/// No current library reads it; the frame is kept raw by the caller.
[<Fact>]
let ``zstd 0.x, the legacy encoding, is refused by name`` () =
    Assert.Equal(Error "zstd 0.x frames (message encoding 2) are not supported", ProcessFrame.decode (frame 2) |> Result.map ignore)

/// POST /api/v1/collector as a real datadog/agent 7.84.0 sent it (hostname
/// "probe"): what current agents put on the wire is encoding 4.
[<Fact>]
let ``a frame from a real agent`` () =
    let data = File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "Fixtures", "frames", "frame_agent_7_84_0.bin"))

    match ProcessFrame.decode data with
    | Error e -> Assert.Fail e
    | Ok decoded ->
        Assert.Equal(4uy, decoded.Header.Encoding)
        Assert.Equal(12uy, decoded.Header.Type)
        let proc = decoded.Body :?> CollectorProc
        Assert.Equal("probe", proc.HostName)
        Assert.NotEmpty proc.Processes
        Assert.Contains(proc.Processes, (fun p -> p.Command.Args |> Seq.exists (fun arg -> arg.Contains "agent")))

[<Fact>]
let ``header problems are named as the agent's library names them`` () =
    Assert.Equal(Error "invalid message length: 3", ProcessFrame.decode [| 3uy; 0uy; 12uy |] |> Result.map ignore)
    Assert.Equal(Error "invalid message version: 9", ProcessFrame.decode (Array.append [| 9uy |] (Array.zeroCreate 16)) |> Result.map ignore)
    Assert.Equal(Error "unhandled message type: 99", ProcessFrame.decode (Array.append [| 3uy; 0uy; 99uy |] (Array.zeroCreate 14)) |> Result.map ignore)
    Assert.Equal(Error "unknown message encoding: 3", ProcessFrame.decode (Array.append [| 3uy; 3uy; 12uy |] (Array.zeroCreate 14)) |> Result.map ignore)

[<Fact>]
let ``what encode writes, decode reads`` () =
    let body = ResCollector()
    body.Message <- "ok"

    match ProcessFrame.decode (ProcessFrame.encode 23uy 5L body) with
    | Ok decoded ->
        Assert.Equal(23uy, decoded.Header.Type)
        Assert.Equal(5L, decoded.Header.Timestamp)
        Assert.Equal("ok", (decoded.Body :?> ResCollector).Message)
    | Error e -> Assert.Fail e
