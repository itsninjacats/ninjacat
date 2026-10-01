/// agent-intake.logs.<site> — the agent's TCP transport for logs.
///
///   agent config: logs_config.logs_dd_url, when logs_config.use_http is not
///   forced and HTTPS was unreachable at agent start-up.
///
/// A raw TCP (or TLS) byte stream, not HTTP. The agent sends and never reads:
/// no handshake, no acknowledgement. Kestrel owns the socket; this file owns
/// the bytes.
///
/// Two framings exist, chosen by the agent's `dev_mode_use_proto` for the
/// whole process, so decided here once per connection:
///
///   length-prefixed (the agent's default):
///       [4-byte big-endian length][<api key> ][protobuf Log]
///       the length counts everything after itself.
///   lines:
///       <api key> <payload>\n
///
/// Both glue exactly one space between the key and the payload.
module NinjaCat.Api.Intake.TcpLogs

open System
open System.Buffers.Binary
open System.Buffers
open System.IO
open System.IO.Pipelines
open System.Threading
open System.Threading.Tasks
open Microsoft.AspNetCore.Connections
open Microsoft.AspNetCore.Connections.Features
open Microsoft.Extensions.Logging
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

type Framing =
    | Lines
    | LengthPrefixed

/// A decoded row waits at most this long, or for this many others.
let private maxBatch = 500
let private flushInterval = TimeSpan.FromSeconds 1.0

/// A Datadog API key is 32 hex characters.
let private keyLength = 32

/// A frame declaring more than this is refused before anything is allocated.
let private maxFrameLength = 1 <<< 20

let private isHex (b: byte) =
    (b >= '0'B && b <= '9'B) || (b >= 'a'B && b <= 'f'B) || (b >= 'A'B && b <= 'F'B)

/// The framing of a connection, from its first bytes: 32 hex characters and
/// a space can only be the line form. Anything else, or too few bytes, is
/// read as the length-prefixed default.
let detectFraming (first: byte[]) : Framing =
    if first.Length > keyLength && first[keyLength] = ' 'B && first |> Array.take keyLength |> Array.forall isHex then
        Lines
    else
        LengthPrefixed

/// What one frame gave.
type FrameResult =
    | Nothing
    | LogLine of LogRow
    | Undecodable of RawPayloadRow
    /// The key is not ours. There is no channel to answer a 403 on, and
    /// accepting bytes under an unknown key is not ours to decide quietly:
    /// the connection is closed.
    | UnknownKey

let private fromProto (tenant: string) (item: Pb.Log) (arrival: DateTime) : LogRow =
    // The proto has no unit on `timestamp`; milliseconds, as every other
    // numeric log timestamp on this path.
    let time, source =
        if item.Timestamp > 0L then Time.fromUnixMillis item.Timestamp, "tcp_proto_ms" else arrival, "arrival"

    { TenantID = tenant
      Timestamp = time
      Host = item.Hostname
      Service = item.Service
      Source = item.Source
      Status = item.Status
      Message = item.Message
      Tags = Tags.toMultiMap item.Tags
      Attributes = ""
      TimestampSource = source }

/// The line form is not documented as JSON. A line that is not a JSON log
/// becomes a row whose message is the whole line, not an error.
let private wholeLine (tenant: string) (payload: byte[]) : LogRow =
    { TenantID = tenant
      Timestamp = DateTime.UtcNow
      Host = ""
      Service = ""
      Source = ""
      Status = ""
      Message = Text.utf8 payload
      Tags = Map.empty
      Attributes = ""
      TimestampSource = "arrival" }

/// One frame, without its delimiter or length prefix: `<api key> <payload>`.
let handleFrame (store: ApiKeys.Store) (log: ILogger) (remote: string) (framing: Framing) (frame: byte[]) : FrameResult =
    if frame.Length = 0 then
        Nothing
    else
        match Array.IndexOf(frame, ' 'B) with
        | -1 ->
            // No key, so no tenant to keep it under: dropped, not stored.
            log.LogWarning("[logs-tcp] {Remote}: frame has no api key prefix, dropped ({Bytes} B)", remote, frame.Length)
            Nothing
        | space ->
            match store.Lookup(Text.utf8 frame[.. space - 1]) with
            | None ->
                log.LogWarning("[logs-tcp] {Remote}: unknown API key, closing connection", remote)
                UnknownKey
            | Some key ->
                let payload = frame[space + 1 ..]
                let arrival = DateTime.UtcNow

                match framing with
                | LengthPrefixed ->
                    try
                        LogLine(fromProto key.TenantID (Pb.Log.Parser.ParseFrom payload) arrival)
                    with e ->
                        Undecodable
                            { TenantID = key.TenantID
                              ReceivedAt = arrival
                              Intake = "logs-tcp"
                              Reason = "decode_error"
                              Method = "TCP"
                              Host = ""
                              Path = ""
                              Query = Map.empty
                              ContentType = "application/x-protobuf"
                              ContentEncoding = ""
                              // No Host header on this wire; the peer is what identifies the sender.
                              Headers = Map [ "Remote-Addr", remote ]
                              Body = payload
                              BodyBytes = uint64 payload.Length
                              Note = "length-prefixed proto: " + e.Message }
                | Lines ->
                    let noDefaults: Routers.Logs.BatchDefaults = { Source = ""; Service = ""; Host = ""; Tags = [] }

                    match Json.tryParse payload |> Result.mapError ignore |> Result.bind (Routers.Logs.decodeItem >> Result.mapError ignore) with
                    | Ok item -> LogLine(Routers.Logs.toRow key.TenantID item arrival noDefaults)
                    | Error() -> LogLine(wholeLine key.TenantID payload)

/// Complete frames at the start of `buffer`, and how many bytes they took.
/// An error means the stream can no longer be trusted.
let takeFrames (framing: Framing) (buffer: ReadOnlySpan<byte>) : Result<byte[] list * int, string> =
    let frames = ResizeArray<byte[]>()
    let mutable consumed = 0
    let mutable error = None
    let mutable more = true

    while more && error.IsNone do
        let rest = buffer.Slice consumed

        match framing with
        | Lines ->
            match rest.IndexOf '\n'B with
            | -1 -> more <- false
            | newline ->
                frames.Add(rest.Slice(0, newline).ToArray())
                consumed <- consumed + newline + 1
        | LengthPrefixed ->
            if rest.Length < 4 then
                more <- false
            else
                let length = BinaryPrimitives.ReadUInt32BigEndian rest

                if length > uint32 maxFrameLength then
                    error <- Some $"frame declares {length} bytes, over the {maxFrameLength} byte limit"
                elif rest.Length < 4 + int length then
                    more <- false
                else
                    frames.Add(rest.Slice(4, int length).ToArray())
                    consumed <- consumed + 4 + int length

    match error with
    | Some e -> Error e
    | None -> Ok(List.ofSeq frames, consumed)

/// Owns one connection: decides its framing, then reads frames and stores
/// what they give, in batches, until the peer leaves or a key is unknown.
let private handleConnection (deps: Deps) (remote: string) (input: PipeReader) (serverStopping: CancellationToken) : Task =
    task {
        let gate = obj ()
        let logs = ResizeArray<LogRow>()
        let raws = ResizeArray<RawPayloadRow>()

        // One write per message would turn a busy connection into a storm of
        // single-row batches.
        let flush () =
            lock gate (fun () ->
                Sink.write deps.Sink Logs.table (logs.ToArray())
                Sink.write deps.Sink RawPayloads.table (raws.ToArray())
                logs.Clear()
                raws.Clear())

        use timer = new Timer((fun _ -> flush ()), null, flushInterval, flushInterval)

        try
            try
                let mutable framing = None
                let mutable opened = true
                let connectedAt = DateTime.UtcNow

                while opened do
                    // Everything received and not yet consumed, however many
                    // reads it took to arrive.
                    let! read = input.ReadAsync serverStopping
                    let received = read.Buffer.ToArray()
                    let mutable consumed = 0

                    // The framing shows in the first 33 bytes. A sender
                    // slower than that is taken for the default.
                    if framing.IsNone && (received.Length > keyLength || DateTime.UtcNow - connectedAt > flushInterval) then
                        framing <- Some(detectFraming received)

                    match framing with
                    | None -> ()
                    | Some framing ->
                        match takeFrames framing (ReadOnlySpan received) with
                        | Error e ->
                            deps.Log.LogWarning("[logs-tcp] {Remote}: closing connection: {Error}", remote, e)
                            opened <- false
                        | Ok(frames, taken) ->
                            consumed <- taken

                            for frame in frames do
                                if opened then
                                    match handleFrame deps.Store deps.Log remote framing frame with
                                    | Nothing -> ()
                                    | LogLine row -> lock gate (fun () -> logs.Add row)
                                    | Undecodable row -> lock gate (fun () -> raws.Add row)
                                    | UnknownKey -> opened <- false

                            if lock gate (fun () -> logs.Count >= maxBatch || raws.Count >= maxBatch) then
                                flush ()

                    // What was not a whole frame stays in the pipe for the next read.
                    input.AdvanceTo(read.Buffer.GetPosition(int64 consumed), read.Buffer.End)

                    if read.IsCompleted then
                        opened <- false
            with
            | :? OperationCanceledException -> ()
            | :? IOException as e -> deps.Log.LogDebug("[logs-tcp] {Remote}: connection ended: {Error}", remote, e.Message)
        finally
            // Rows already decoded are kept, whatever ended the connection.
            flush ()
    }

/// One connection, as Kestrel hands it over: Kestrel listens, accepts and,
/// when the endpoint has a certificate, speaks TLS.
type Connection(deps: Deps) =
    inherit ConnectionHandler()

    override _.OnConnectedAsync(connection: ConnectionContext) : Task =
        // An agent holds its connection open for good, so a stopping server
        // has to be the one to leave. This token is the server's request; the
        // connection's own (ConnectionClosed) fires when the PEER closes,
        // which can be before its last bytes were read.
        let lifetime = connection.Features.Get<IConnectionLifetimeNotificationFeature>()

        handleConnection deps (string connection.RemoteEndPoint) connection.Transport.Input lifetime.ConnectionClosedRequested
