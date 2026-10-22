//! agent-intake.logs.<site> — the agent's TCP transport for logs.
//!
//!   agent config: logs_config.logs_dd_url, when logs_config.use_http is not forced and HTTPS was
//!   unreachable at agent start-up.
//!
//! A raw TCP (or TLS) byte stream, not HTTP. The agent sends and never reads: no handshake, no
//! acknowledgement. It is served beside the logs host's HTTP listener, on `NINJACAT_LOGS_TCP_ADDR`
//! (`:10516`, `off` for none), with TLS when `NINJACAT_TLS_CERT` and `NINJACAT_TLS_KEY` name PEM
//! files.
//!
//! Two framings exist, chosen by the agent's `dev_mode_use_proto` for the whole process, so decided
//! here once per connection:
//!
//!   length-prefixed (the agent's default):
//!       [4-byte big-endian length][<api key> ][protobuf Log]
//!       the length counts everything after itself.
//!   lines:
//!       <api key> <payload>\n
//!
//! Both glue exactly one space between the key and the payload.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use ninjapaws_proto::pb;
use ninjapaws_storage::Sink;
use ninjapaws_storage::column::{self, Blob};
use ninjapaws_storage::rows::Tags;
use ninjapaws_storage::rows::logs::Log;
use ninjapaws_storage::rows::raw::RawPayload;
use prost::Message;
use tako::types::BoxError;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_util::sync::CancellationToken;

use crate::json;
use crate::keys::Store;
use crate::routers::logs::{self, BatchDefaults};
use crate::server::{Intake, listen_on, var_or};
use crate::wire::{tags, text, time};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    Lines,
    LengthPrefixed,
}

/// A decoded row waits at most this long, or for this many others.
const MAX_BATCH: usize = 500;
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// A Datadog API key is 32 hex characters.
const KEY_LENGTH: usize = 32;

/// A frame declaring more than this is refused before anything is allocated.
const MAX_FRAME_LENGTH: usize = 1 << 20;

/// The framing of a connection, from its first bytes: 32 hex characters and a space can only be
/// the line form. Anything else, or too few bytes, is read as the length-prefixed default.
pub fn detect_framing(first: &[u8]) -> Framing {
    if first.len() > KEY_LENGTH && first[KEY_LENGTH] == b' ' && first[..KEY_LENGTH].iter().all(u8::is_ascii_hexdigit) {
        Framing::Lines
    } else {
        Framing::LengthPrefixed
    }
}

/// What one frame gave.
#[derive(Debug)]
pub enum FrameResult {
    Nothing,
    LogLine(Log),
    Undecodable(RawPayload),
    /// The key is not ours. There is no channel to answer a 403 on, and accepting bytes under an
    /// unknown key is not ours to decide quietly: the connection is closed.
    UnknownKey,
}

fn from_proto(tenant: &str, item: pb::Log, arrival: DateTime<Utc>) -> Log {
    // The proto has no unit on `timestamp`; milliseconds, as every other numeric log timestamp on
    // this path.
    let (timestamp, source) = if item.timestamp > 0 {
        (time::from_unix_millis(item.timestamp), "tcp_proto_ms")
    } else {
        (arrival, "arrival")
    };
    Log {
        tenant_id: tenant.into(),
        timestamp,
        host: item.hostname,
        service: item.service,
        source: item.source,
        status: item.status,
        message: item.message,
        tags: tags::to_multi_map(item.tags),
        attributes: column::json_object(""),
        timestamp_source: source.into(),
    }
}

/// The line form is not documented as JSON. A line that is not a JSON log becomes a row whose
/// message is the whole line, not an error.
fn whole_line(tenant: &str, payload: &[u8]) -> Log {
    Log {
        tenant_id: tenant.into(),
        timestamp: Utc::now(),
        host: String::new(),
        service: String::new(),
        source: String::new(),
        status: String::new(),
        message: text::utf8(payload),
        tags: Tags::new(),
        attributes: column::json_object(""),
        timestamp_source: "arrival".into(),
    }
}

/// One frame, without its delimiter or length prefix: `<api key> <payload>`.
pub fn handle_frame(store: &Store, remote: &str, framing: Framing, frame: &[u8]) -> FrameResult {
    if frame.is_empty() {
        return FrameResult::Nothing;
    }
    let Some(space) = frame.iter().position(|b| *b == b' ') else {
        // No key, so no tenant to keep it under: dropped, not stored.
        tracing::warn!("[logs-tcp] {remote}: frame has no api key prefix, dropped ({} B)", frame.len());
        return FrameResult::Nothing;
    };
    let Some(key) = store.lookup(&text::utf8(&frame[..space])) else {
        tracing::warn!("[logs-tcp] {remote}: unknown API key, closing connection");
        return FrameResult::UnknownKey;
    };
    let payload = &frame[space + 1..];
    let arrival = Utc::now();
    match framing {
        Framing::LengthPrefixed => match pb::Log::decode(payload) {
            Ok(item) => FrameResult::LogLine(from_proto(&key.tenant_id, item, arrival)),
            Err(e) => FrameResult::Undecodable(RawPayload {
                tenant_id: key.tenant_id,
                received_at: arrival,
                intake: "logs-tcp".into(),
                reason: "decode_error".into(),
                method: "TCP".into(),
                host: String::new(),
                path: String::new(),
                query: BTreeMap::new(),
                content_type: "application/x-protobuf".into(),
                content_encoding: String::new(),
                // No Host header on this wire; the peer is what identifies the sender.
                headers: BTreeMap::from([("Remote-Addr".to_string(), remote.to_string())]),
                body: Blob(payload.to_vec()),
                body_bytes: payload.len() as u64,
                note: format!("length-prefixed proto: {e}"),
            }),
        },
        Framing::Lines => {
            let item = json::try_parse(payload).ok().and_then(|value| logs::decode_item(&value).ok());
            match item {
                Some(item) => {
                    FrameResult::LogLine(logs::to_row(&key.tenant_id, item, arrival, &BatchDefaults::default()))
                }
                None => FrameResult::LogLine(whole_line(&key.tenant_id, payload)),
            }
        }
    }
}

/// Complete frames at the start of `buffer`, and how many bytes they took. An error means the
/// stream can no longer be trusted.
pub fn take_frames(framing: Framing, buffer: &[u8]) -> Result<(Vec<&[u8]>, usize), String> {
    let mut frames = Vec::new();
    let mut consumed = 0;
    loop {
        let rest = &buffer[consumed..];
        match framing {
            Framing::Lines => match rest.iter().position(|b| *b == b'\n') {
                None => break,
                Some(newline) => {
                    frames.push(&rest[..newline]);
                    consumed += newline + 1;
                }
            },
            Framing::LengthPrefixed => {
                let Some(prefix) = rest.first_chunk::<4>() else { break };
                let length = u32::from_be_bytes(*prefix) as usize;
                if length > MAX_FRAME_LENGTH {
                    return Err(format!("frame declares {length} bytes, over the {MAX_FRAME_LENGTH} byte limit"));
                }
                if rest.len() < 4 + length {
                    break;
                }
                frames.push(&rest[4..4 + length]);
                consumed += 4 + length;
            }
        }
    }
    Ok((frames, consumed))
}

/// Owns one connection: decides its framing, then reads frames and stores what they give, in
/// batches, until the peer leaves, a key is unknown or the process stops.
async fn handle_connection(
    store: Arc<Store>,
    sink: Sink,
    remote: String,
    mut input: impl AsyncRead + Unpin,
    stopping: CancellationToken,
) {
    let mut logs = Vec::new();
    let mut raws = Vec::new();
    // One write per message would turn a busy connection into a storm of single-row batches.
    let flush = |logs: &mut Vec<Log>, raws: &mut Vec<RawPayload>| {
        sink.write(std::mem::take(logs));
        sink.write(std::mem::take(raws));
    };
    let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + FLUSH_INTERVAL, FLUSH_INTERVAL);
    // Everything received and not yet consumed, however many reads it took to arrive.
    let mut received = Vec::new();
    let mut framing = None;
    let connected_at = Instant::now();
    let mut open = true;
    while open {
        tokio::select! {
            // An agent holds its connection open for good, so a stopping process has to be the
            // one to leave.
            _ = stopping.cancelled() => open = false,
            _ = ticks.tick() => flush(&mut logs, &mut raws),
            read = input.read_buf(&mut received) => match read {
                Ok(0) => open = false,
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("[logs-tcp] {remote}: connection ended: {e}");
                    open = false;
                }
            },
        }
        // The framing shows in the first 33 bytes. A sender slower than that is taken for the
        // default.
        if framing.is_none() && (received.len() > KEY_LENGTH || connected_at.elapsed() > FLUSH_INTERVAL) {
            framing = Some(detect_framing(&received));
        }
        let Some(framing) = framing else { continue };
        let taken = match take_frames(framing, &received) {
            Err(e) => {
                tracing::warn!("[logs-tcp] {remote}: closing connection: {e}");
                break;
            }
            Ok((frames, taken)) => {
                for frame in frames {
                    match handle_frame(&store, &remote, framing, frame) {
                        FrameResult::Nothing => {}
                        FrameResult::LogLine(row) => logs.push(row),
                        FrameResult::Undecodable(row) => raws.push(row),
                        FrameResult::UnknownKey => {
                            open = false;
                            break;
                        }
                    }
                }
                taken
            }
        };
        // What was not a whole frame waits for the next read.
        received.drain(..taken);
        if logs.len() >= MAX_BATCH || raws.len() >= MAX_BATCH {
            flush(&mut logs, &mut raws);
        }
    }
    // Rows already decoded are kept, whatever ended the connection.
    flush(&mut logs, &mut raws);
}

/// What makes the listener speak TLS: the certificate chain and key in two PEM files.
pub fn tls_acceptor(cert_file: &str, key_file: &str) -> Result<TlsAcceptor, BoxError> {
    let chain = CertificateDer::pem_file_iter(cert_file)?.collect::<Result<Vec<_>, _>>()?;
    let key = PrivateKeyDer::from_pem_file(key_file)?;
    let config = ServerConfig::builder().with_no_client_auth().with_single_cert(chain, key)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Accepts connections on `listener` until the process stops, each served as a task of its own.
pub fn serve(listener: TcpListener, tls: Option<TlsAcceptor>, intake: &Intake) {
    let (keys, sink, stopping, tasks) =
        (intake.keys.clone(), intake.sink.clone(), intake.stopping.clone(), intake.tasks.clone());
    intake.tasks.spawn(async move {
        loop {
            let (stream, peer) = tokio::select! {
                _ = stopping.cancelled() => return,
                accepted = listener.accept() => match accepted {
                    Ok(accepted) => accepted,
                    Err(e) => {
                        tracing::warn!("[logs-tcp] accept failed: {e}");
                        continue;
                    }
                },
            };
            let (keys, sink, stopping, tls) = (keys.clone(), sink.clone(), stopping.clone(), tls.clone());
            let remote = peer.to_string();
            tasks.spawn(async move {
                match tls {
                    None => handle_connection(keys, sink, remote, stream, stopping).await,
                    Some(tls) => match tls.accept(stream).await {
                        Ok(stream) => handle_connection(keys, sink, remote, stream, stopping).await,
                        Err(e) => tracing::debug!("[logs-tcp] {remote}: TLS handshake failed: {e}"),
                    },
                }
            });
        }
    });
}

/// The listener as the environment configures it, started beside the HTTP one; nothing when
/// `NINJACAT_LOGS_TCP_ADDR` is `off`. Called inside the runtime.
pub fn listen(intake: &Intake) -> Result<(), BoxError> {
    let addr = var_or("NINJACAT_LOGS_TCP_ADDR", ":10516");
    if addr == "off" {
        return Ok(());
    }
    let (cert_file, key_file) = (var_or("NINJACAT_TLS_CERT", ""), var_or("NINJACAT_TLS_KEY", ""));
    let tls =
        if cert_file.is_empty() && key_file.is_empty() { None } else { Some(tls_acceptor(&cert_file, &key_file)?) };
    let listener = std::net::TcpListener::bind(listen_on(&addr))?;
    listener.set_nonblocking(true)?;
    let listener = TcpListener::from_std(listener)?;
    tracing::info!("logs over TCP listening on {addr}{}", if tls.is_some() { " (TLS)" } else { "" });
    serve(listener, tls, intake);
    Ok(())
}
