//! Dumps every incoming request to disk. A debugging tool, switched on with DEBUG=true, and not
//! part of the request path.
//!
//! This is how the Datadog protocol was worked out: point an agent here and read what actually
//! arrives — decompressed, JSON pretty-printed, protobuf walked field by field without a schema,
//! a hexdump as the last resort. Each request gets a file in NINJACAT_CAPTURE_DIR (default
//! `captures`) and a line in its `index.log`.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::{Buf, Bytes};
use chrono::{DateTime, SecondsFormat, Utc};
use http::StatusCode;
use http_body_util::{BodyExt, Limited};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use prost::encoding::WireType;
use tako::body::TakoBody;
use tako::extractors::ipaddr::IpAddr;
use tako::middleware::Next;
use tako::types::{Request, Response};

use crate::raw::{KEY_PARAMETERS, path_without_key};
use crate::server::{Settings, query_pairs, state, status};
use crate::{body, json};

/// Text a person can read: valid UTF-8 with no control characters but newlines and tabs.
fn printable(data: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(data).ok()?;
    text.chars().all(|c| matches!(c, '\n' | '\r' | '\t') || !c.is_control()).then_some(text)
}

fn hex_preview(data: &[u8]) -> String {
    let hex: String = data.iter().take(16).map(|b| format!("{b:02x}")).collect();
    if data.len() > 16 { hex + "..." } else { hex }
}

/// Walks protobuf's wire format with no schema: `field_number: value` per line, nested messages
/// indented. False when the bytes are not protobuf.
fn dump_proto_into(out: &mut String, data: &[u8], depth: usize) -> bool {
    if data.is_empty() || depth > 8 {
        return false;
    }
    let pad = " ".repeat(depth * 2);
    let mut input = data;
    while input.has_remaining() {
        let Ok((field, wire_type)) = prost::encoding::decode_key(&mut input) else { return false };
        match wire_type {
            WireType::Varint => {
                let Ok(value) = prost::encoding::decode_varint(&mut input) else { return false };
                writeln!(out, "{pad}{field}: {value}").unwrap();
            }
            WireType::SixtyFourBit => {
                if input.remaining() < 8 {
                    return false;
                }
                let bits = input.get_u64_le();
                writeln!(out, "{pad}{field}: {bits} (f64 {})", f64::from_bits(bits)).unwrap();
            }
            WireType::ThirtyTwoBit => {
                if input.remaining() < 4 {
                    return false;
                }
                let bits = input.get_u32_le();
                writeln!(out, "{pad}{field}: {bits} (f32 {})", f32::from_bits(bits)).unwrap();
            }
            WireType::LengthDelimited => {
                let Ok(length) = prost::encoding::decode_varint(&mut input) else { return false };
                let Some(value) = usize::try_from(length).ok().and_then(|n| input.get(..n)) else { return false };
                input.advance(value.len());

                // A length-delimited field is text, a nested message, or bytes; tried in that
                // order.
                let mut nested = String::new();
                if value.is_empty() {
                    writeln!(out, "{pad}{field}: \"\"").unwrap();
                } else if let Some(text) = printable(value) {
                    writeln!(out, "{pad}{field}: {}", json::quoted(text)).unwrap();
                } else if dump_proto_into(&mut nested, value, depth + 1) {
                    write!(out, "{pad}{field}: {{\n{nested}{pad}}}\n").unwrap();
                } else {
                    writeln!(out, "{pad}{field}: <{} B> {}", value.len(), hex_preview(value)).unwrap();
                }
            }
            WireType::StartGroup | WireType::EndGroup => return false,
        }
    }
    true
}

/// The bytes as a schema-less protobuf dump, or None when they are not protobuf.
pub fn dump_proto(data: &[u8]) -> Option<String> {
    let mut out = String::new();
    (dump_proto_into(&mut out, data, 0) && !out.is_empty()).then_some(out)
}

/// The classic hexdump: offset, sixteen bytes, their ASCII. First 4096 bytes.
pub fn hex_dump(data: &[u8]) -> String {
    const LIMIT: usize = 4096;
    let mut out = String::new();
    for (row_number, row) in data[..data.len().min(LIMIT)].chunks(16).enumerate() {
        write!(out, "{:08x}  ", row_number * 16).unwrap();
        for i in 0..16 {
            match row.get(i) {
                Some(b) => write!(out, "{b:02x} ").unwrap(),
                None => out.push_str("   "),
            }
        }
        out.push_str(" |");
        out.extend(row.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }));
        out.push_str("|\n");
    }
    if data.len() > LIMIT {
        writeln!(out, "... (cut at {LIMIT} B)").unwrap();
    }
    out
}

/// A body as something to read: JSON, then protobuf, then text, then hex.
pub fn render(body: &[u8]) -> String {
    if body.is_empty() {
        return "(empty body)".into();
    }
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
        return serde_json::to_string_pretty(&value).unwrap();
    }
    if let Some(proto) = dump_proto(body) {
        return "(protobuf, walked without a schema: field_number: value)\n\n".to_string() + &proto;
    }
    match printable(body) {
        Some(text) => text.to_string(),
        None => hex_dump(body),
    }
}

/// A path as part of a file name.
pub fn slug(path: &str) -> String {
    match path.trim_matches('/') {
        "" => "root".into(),
        trimmed => trimmed.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect(),
    }
}

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
static INDEX_LOCK: Mutex<()> = Mutex::new(());

/// Headers that carry a key are not written to disk.
const SECRET_HEADERS: [&str; 4] = ["dd-api-key", "dd-client-token", "authorization", "dd-application-key"];

/// What `Uri.EscapeDataString` leaves alone: RFC 3986's unreserved characters.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// The query string with the value of a key parameter replaced.
fn query_without_keys(parts: &http::request::Parts) -> String {
    let shown: Vec<String> = query_pairs(&parts.uri)
        .into_iter()
        .map(|(key, value)| {
            let value = if KEY_PARAMETERS.contains(&key.to_lowercase().as_str()) { "<redacted>".into() } else { value };
            format!("{}={}", utf8_percent_encode(&key, UNRESERVED), utf8_percent_encode(&value, UNRESERVED))
        })
        .collect();
    if shown.is_empty() { String::new() } else { format!("?{}", shown.join("&")) }
}

fn or_dash(text: &str) -> &str {
    if text.is_empty() { "-" } else { text }
}

/// The peer's address, as the transport gives it: no forwarding header is believed here.
fn client(parts: &http::request::Parts) -> String {
    match IpAddr::resolve(&parts.extensions, &http::HeaderMap::new()) {
        Ok(IpAddr(ip)) => ip.to_canonical().to_string(),
        Err(_) => String::new(),
    }
}

/// Writes one request to its own file and adds a line to index.log. Returns the file's path.
pub fn write(
    directory: &Path,
    parts: &http::request::Parts,
    raw: &[u8],
    decoded: &[u8],
    status: u16,
    started: DateTime<Utc>,
) -> io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let header = |name| parts.headers.get(name).map(|v: &http::HeaderValue| String::from_utf8_lossy(v.as_bytes()));
    let encoding = header(http::header::CONTENT_ENCODING).unwrap_or_default();
    let host =
        header(http::header::HOST).or_else(|| parts.uri.authority().map(|a| a.as_str().into())).unwrap_or_default();
    let number = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let stamp = started.format("%Y%m%dT%H%M%S%.3f");
    let method = &parts.method;
    let path = path_without_key(parts.uri.path());
    let name = format!("{stamp}_{method}_{}_{number:04}.txt", slug(&path));
    let file = directory.join(&name);

    let took = (Utc::now() - started).num_microseconds().unwrap_or_default() as f64 / 1000.0;
    let mut out = String::new();
    writeln!(out, "# time:     {}", started.to_rfc3339_opts(SecondsFormat::Micros, true)).unwrap();
    writeln!(out, "# client:   {}", client(parts)).unwrap();
    writeln!(out, "# status:   {status}").unwrap();
    writeln!(out, "# took:     {took:.1} ms").unwrap();
    writeln!(out, "# body:     {} B raw -> {} B decompressed ({})\n", raw.len(), decoded.len(), or_dash(&encoding))
        .unwrap();
    writeln!(out, "{method} {path}{} {:?}", query_without_keys(parts), parts.version).unwrap();
    writeln!(out, "Host: {host}").unwrap();
    for (name, value) in &parts.headers {
        if name != http::header::HOST {
            let value = if SECRET_HEADERS.contains(&name.as_str()) {
                "<redacted>".into()
            } else {
                String::from_utf8_lossy(value.as_bytes())
            };
            writeln!(out, "{name}: {value}").unwrap();
        }
    }
    out.push_str("\n---------------- BODY ----------------\n");
    out.push_str(&render(decoded));
    out.push('\n');
    fs::write(&file, out)?;

    let time = started.format("%Y-%m-%dT%H:%M:%SZ");
    let line = format!(
        "{time}\t{method}\t{path}\t{status}\t{}\t{}\t{}\t{name}\n",
        raw.len(),
        decoded.len(),
        or_dash(&encoding)
    );
    let _guard = INDEX_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    OpenOptions::new().create(true).append(true).open(directory.join("index.log"))?.write_all(line.as_bytes())?;

    Ok(file)
}

/// The capture lab, a router middleware (DEBUG=true): every request is dumped into `directory`
/// once it has been answered, the ones nothing serves above all. The body is
/// read here and handed on whole.
pub async fn dump(directory: PathBuf, req: Request, next: Next) -> Response {
    let started = Utc::now();
    let limit = state::<Settings>(req.extensions()).map_or(0, |settings| settings.max_body_bytes);
    let (parts, body) = req.into_parts();
    let raw: Bytes = match Limited::new(body, limit).collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return status(StatusCode::PAYLOAD_TOO_LARGE),
    };
    let response = next.run(Request::from_parts(parts.clone(), TakoBody::from(raw.clone()))).await;

    let encoding = parts.headers.get(http::header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let decoded = body::decompress(encoding, raw.clone());
    if let Err(e) = write(&directory, &parts, &raw, &decoded, response.status().as_u16(), started) {
        tracing::warn!("capture not written: {e}");
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_pretty_printed_text_is_left_alone_nothing_says_so() {
        assert_eq!("(empty body)", render(b""));
        assert!(render(br#"{"a":1}"#).contains("\"a\": 1"));
        assert_eq!("plain text, not JSON", render(b"plain text, not JSON"));
    }

    #[test]
    fn json_keeps_its_order_and_its_numbers() {
        assert_eq!("{\n  \"b\": 1.50,\n  \"a\": 2\n}", render(br#"{"b":1.50,"a":2}"#));
    }

    #[test]
    fn protobuf_is_walked_without_a_schema() {
        // field 1 = "hi", field 2 = 150, field 3 = { field 1 = 1.5 as a double }
        let message = [0x0a, 2, b'h', b'i', 0x10, 0x96, 0x01, 0x1a, 9, 0x09, 0, 0, 0, 0, 0, 0, 0xf8, 0x3f];
        let dump = dump_proto(&message).expect("not recognised as protobuf");
        assert!(dump.contains("1: \"hi\""), "{dump}");
        assert!(dump.contains("2: 150"), "{dump}");
        assert!(dump.contains("3: {"), "{dump}");
        assert!(dump.contains("(f64 1.5)"), "{dump}");
        assert!(render(&message).starts_with("(protobuf"));
    }

    #[test]
    fn bytes_that_are_nothing_else_become_a_hexdump() {
        let dump = render(&[0xff, 0xfe, 0x00, b'A']);
        assert!(dump.starts_with("00000000  ff fe 00 41 "), "{dump}");
        assert!(dump.ends_with("|...A|\n"), "{dump}");
        assert!(hex_dump(&[0xff; 5000]).contains("(cut at 4096 B)"));
    }

    #[test]
    fn a_path_becomes_a_file_name() {
        assert_eq!("api_v2_series", slug("/api/v2/series"));
        assert_eq!("root", slug("/"));
        assert_eq!("v1_input_abc_def", slug("/v1/input/abc-def"));
    }
}
