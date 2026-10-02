module NinjaCat.Api.Intake.Tests.TcpLogsTests

open System
open System.Buffers.Binary
open System.Net
open System.Net.Sockets
open System.Text
open System.Threading
open Google.Protobuf
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Connections
open Microsoft.AspNetCore.Hosting
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.Logging.Abstractions
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

let private store = Replay.testStore ()
let private utf8 (text: string) = Encoding.UTF8.GetBytes text

let private handle (framing: TcpLogs.Framing) (frame: byte[]) =
    TcpLogs.handleFrame store NullLogger.Instance "10.0.0.1:5000" framing frame

let private protoLog (message: string) (timestamp: int64) : byte[] =
    let log = Pb.Log(Message = message, Status = "info", Timestamp = timestamp, Hostname = "h1", Service = "svc", Source = "go")
    log.Tags.Add "env:prod"
    log.ToByteArray()

/// `<key> <payload>` with its 4-byte length in front.
let private prefixed (key: string) (payload: byte[]) : byte[] =
    let body = Array.concat [ utf8 (key + " "); payload ]
    let length = Array.zeroCreate<byte> 4
    BinaryPrimitives.WriteUInt32BigEndian(Span length, uint32 body.Length)
    Array.append length body

[<Fact>]
let ``a key and a space at the start is the line framing, anything else the default`` () =
    Assert.Equal(TcpLogs.Lines, TcpLogs.detectFraming (utf8 (Replay.testKey + " hello\n")))
    Assert.Equal(TcpLogs.LengthPrefixed, TcpLogs.detectFraming (prefixed Replay.testKey (protoLog "x" 0L)))
    Assert.Equal(TcpLogs.LengthPrefixed, TcpLogs.detectFraming (utf8 "short"))
    // 32 characters that are not all hex.
    Assert.Equal(TcpLogs.LengthPrefixed, TcpLogs.detectFraming (utf8 ("zz" + Replay.testKey.Substring 2 + " hello")))

[<Fact>]
let ``frames are cut out as they complete`` () =
    let one = prefixed Replay.testKey (protoLog "a" 0L)
    let stream = Array.concat [ one; one; one[..9] ]

    match TcpLogs.takeFrames TcpLogs.LengthPrefixed (ReadOnlySpan stream) with
    | Ok(frames, consumed) ->
        Assert.Equal(2, frames.Length)
        Assert.Equal(2 * one.Length, consumed)
    | Error e -> Assert.Fail e

    match TcpLogs.takeFrames TcpLogs.Lines (ReadOnlySpan(utf8 "k one\nk two\nk thr")) with
    | Ok(frames, consumed) ->
        Assert.Equal<string list>([ "k one"; "k two" ], frames |> List.map Encoding.UTF8.GetString)
        Assert.Equal(12, consumed)
    | Error e -> Assert.Fail e

[<Fact>]
let ``a frame declaring more than the limit is refused unread`` () =
    Assert.True(Result.isError (TcpLogs.takeFrames TcpLogs.LengthPrefixed (ReadOnlySpan [| 0x7fuy; 0xffuy; 0xffuy; 0xffuy |])))

[<Fact>]
let ``a protobuf log becomes a row with its own timestamp`` () =
    match handle TcpLogs.LengthPrefixed (Array.append (utf8 (Replay.testKey + " ")) (protoLog "hello" 1790151330000L)) with
    | TcpLogs.LogLine row ->
        Assert.Equal(Replay.testTenant, row.TenantID)
        Assert.Equal("hello", row.Message)
        Assert.Equal("h1", row.Host)
        Assert.Equal("tcp_proto_ms", row.TimestampSource)
        Assert.Equal(DateTimeOffset.FromUnixTimeMilliseconds(1790151330000L).UtcDateTime, row.Timestamp)
        Assert.Equal<string[]>([| "prod" |], row.Tags["env"])
    | other -> Assert.Fail $"got %A{other}"

[<Fact>]
let ``a protobuf log without a timestamp is stamped on arrival`` () =
    match handle TcpLogs.LengthPrefixed (Array.append (utf8 (Replay.testKey + " ")) (protoLog "hello" 0L)) with
    | TcpLogs.LogLine row -> Assert.Equal("arrival", row.TimestampSource)
    | other -> Assert.Fail $"got %A{other}"

[<Fact>]
let ``bytes that are not a protobuf log are kept raw, with the peer`` () =
    match handle TcpLogs.LengthPrefixed (utf8 (Replay.testKey + " ÿÿ not proto")) with
    | TcpLogs.Undecodable row ->
        Assert.Equal("logs-tcp", row.Intake)
        Assert.Equal("decode_error", row.Reason)
        Assert.Equal("TCP", row.Method)
        Assert.Equal("10.0.0.1:5000", row.Headers["Remote-Addr"])
        Assert.StartsWith("length-prefixed proto: ", row.Note)
    | other -> Assert.Fail $"got %A{other}"

[<Fact>]
let ``a JSON line is read as a log, any other line is the message`` () =
    match handle TcpLogs.Lines (utf8 (Replay.testKey + """ {"message":"from json","service":"api","status":"error"}""")) with
    | TcpLogs.LogLine row ->
        Assert.Equal("from json", row.Message)
        Assert.Equal("api", row.Service)
        Assert.Equal("error", row.Status)
    | other -> Assert.Fail $"got %A{other}"

    match handle TcpLogs.Lines (utf8 (Replay.testKey + " plain text, with spaces")) with
    | TcpLogs.LogLine row ->
        Assert.Equal("plain text, with spaces", row.Message)
        Assert.Equal("arrival", row.TimestampSource)
    | other -> Assert.Fail $"got %A{other}"

    // JSON, but not shaped like a log: still the whole line.
    match handle TcpLogs.Lines (utf8 (Replay.testKey + """ {"message":5}""")) with
    | TcpLogs.LogLine row -> Assert.Equal("""{"message":5}""", row.Message)
    | other -> Assert.Fail $"got %A{other}"

[<Fact>]
let ``an unknown key closes the connection, a frame without a key is dropped`` () =
    Assert.Equal(TcpLogs.UnknownKey, handle TcpLogs.Lines (utf8 "ffffffffffffffffffffffffffffffff hello"))
    Assert.Equal(TcpLogs.Nothing, handle TcpLogs.Lines (utf8 "nospacehere"))
    Assert.Equal(TcpLogs.Nothing, handle TcpLogs.Lines [||])

/// Sends bytes to a real Kestrel endpoint and waits for the rows.
let private overTcp (payload: byte[]) (expectedRows: int) : CapturingSink =
    let sink = CapturingSink()
    let builder = WebApplication.CreateEmptyBuilder(WebApplicationOptions())
    builder.WebHost.UseKestrelCore() |> ignore
    // What the connection handler asks the container for.
    builder.Services.AddLogging().AddSingleton<ApiKeys.Store>(store).AddSingleton<ISink>(sink) |> ignore

    builder.WebHost.ConfigureKestrel(fun options ->
        options.Listen(IPAddress.Loopback, 0, (fun endpoint -> endpoint.UseConnectionHandler<TcpLogs.Connection>() |> ignore)))
    |> ignore

    use app = builder.Build()
    app.StartAsync().GetAwaiter().GetResult()
    let port = Uri(Seq.head app.Urls).Port

    do
        use client = new TcpClient()
        client.Connect(IPAddress.Loopback, port)
        client.GetStream().Write(payload, 0, payload.Length)

    let deadline = DateTime.UtcNow.AddSeconds 5.0

    while sink.Rows<LogRow>().Length < expectedRows && DateTime.UtcNow < deadline do
        Thread.Sleep 20

    app.StopAsync().Wait(TimeSpan.FromSeconds 5.0) |> ignore
    sink

[<Fact>]
let ``length-prefixed frames over a real connection`` () =
    let payload = Array.concat [ prefixed Replay.testKey (protoLog "one" 0L); prefixed Replay.testKey (protoLog "two" 0L) ]
    let sink = overTcp payload 2
    Assert.Equal<string list>([ "one"; "two" ], sink.Rows<LogRow>() |> List.map _.Message)

[<Fact>]
let ``lines over a real connection, and rows before an unknown key are kept`` () =
    let payload = utf8 $"{Replay.testKey} first\n{Replay.testKey} second\nffffffffffffffffffffffffffffffff third\n{Replay.testKey} never read\n"
    let sink = overTcp payload 2
    Assert.Equal<string list>([ "first"; "second" ], sink.Rows<LogRow>() |> List.map _.Message)
