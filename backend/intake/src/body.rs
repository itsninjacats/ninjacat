//! Request bodies. They arrive compressed three ways, one per agent subsystem: zstd for metrics,
//! gzip for traces, deflate for distribution points. No proxy undoes that for us:
//! Content-Encoding is end to end.

use std::io::Read;

use bytes::Bytes;

/// The most a body may inflate to. Agents send a few megabytes; this only has to stop a few
/// kilobytes that inflate to gigabytes.
pub const MAX_INFLATED_BYTES: usize = 64 * 1024 * 1024;

/// Everything a decoder gives, up to that limit; past it the read fails.
pub fn read_inflated(decoder: impl Read) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    decoder.take(MAX_INFLATED_BYTES as u64 + 1).read_to_end(&mut out)?;
    if out.len() > MAX_INFLATED_BYTES {
        return Err(std::io::Error::other(format!("it inflates past {MAX_INFLATED_BYTES} bytes")));
    }
    Ok(out)
}

fn inflate(encoding: &str, body: &[u8]) -> std::io::Result<Vec<u8>> {
    match encoding {
        // Several frames one after another are one body: the agent compresses each column of a
        // metrics v3 payload as its own frame.
        "zstd" => read_inflated(zstd::stream::read::Decoder::new(body)?),
        "gzip" => read_inflated(flate2::read::MultiGzDecoder::new(body)),
        // The RFC says zlib; some clients send bare deflate.
        "deflate" | "zlib" => read_inflated(flate2::read::ZlibDecoder::new(body))
            .or_else(|_| read_inflated(flate2::read::DeflateDecoder::new(body))),
        _ => Ok(body.to_vec()),
    }
}

/// The body decompressed. An unknown encoding, a body that does not decode, or one that inflates
/// past the limit comes back unchanged: a mislabelled Content-Encoding should not lose a payload
/// the handler could still read, and the handler keeps what it cannot.
pub fn decompress(encoding: &str, body: Bytes) -> Bytes {
    if encoding.is_empty() {
        return body;
    }
    match inflate(encoding, &body) {
        Ok(inflated) => Bytes::from(inflated),
        Err(e) => {
            tracing::warn!("[body] {encoding} decompression failed, passing through: {e}");
            body
        }
    }
}

/// The format of a body: from Content-Type, and when that is missing or says nothing, from the
/// first significant byte.
pub fn is_json(content_type: &str, body: &[u8]) -> bool {
    if content_type.contains("json") {
        true
    } else if content_type.contains("protobuf") || content_type.contains("octet-stream") {
        false
    } else {
        matches!(body.iter().find(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n')), Some(b'{' | b'['))
    }
}

/// One part of a multipart body.
#[derive(Clone, Debug, Default)]
pub struct Part {
    /// The form field's name, or "".
    pub name: String,
    /// The uploaded file's name, or "" for a plain field.
    pub file_name: String,
    pub content_type: String,
    pub data: Bytes,
}

/// The boundary of a multipart Content-Type, or None when it is not one.
pub fn boundary(content_type: &str) -> Option<String> {
    multer::parse_boundary(content_type).ok().filter(|b| !b.is_empty())
}

/// The parts read, in order and with repeats, and why reading stopped before the end of the body,
/// when it did.
async fn read_parts(boundary: &str, body: Bytes) -> (Vec<Part>, Option<String>) {
    let stream = futures::stream::once(async move { Ok::<Bytes, std::io::Error>(body) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut parts = Vec::new();
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let name = field.name().unwrap_or_default().to_string();
                let file_name = field.file_name().unwrap_or_default().to_string();
                let content_type = field.content_type().map(|m| m.to_string()).unwrap_or_default();
                match field.bytes().await {
                    Ok(data) => parts.push(Part { name, file_name, content_type, data }),
                    Err(e) => return (parts, Some(e.to_string())),
                }
            }
            Ok(None) => return (parts, None),
            Err(e) => return (parts, Some(e.to_string())),
        }
    }
}

/// Every part of a multipart body. An Err when the content type is not multipart, names no
/// boundary, or the body breaks off: the caller then keeps the request whole instead of storing
/// half a submission.
pub async fn try_parts(content_type: &str, body: Bytes) -> Result<Vec<Part>, String> {
    let media = content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    if media.is_empty() {
        return Err("the content type is not a media type".into());
    }
    if !media.starts_with("multipart/") {
        return Err(format!("the content type is {media}, not multipart"));
    }
    let Some(boundary) = boundary(content_type) else {
        return Err("the content type is multipart without a boundary".into());
    };
    match read_parts(&boundary, body).await {
        (parts, None) => Ok(parts),
        (_, Some(problem)) => Err(problem),
    }
}

/// The parts before the break, for an upload worth keeping in part (a flare whose last fields
/// were cut off still has its archive).
pub async fn parts(boundary: &str, body: Bytes) -> Vec<Part> {
    read_parts(boundary, body).await.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn concatenated_zstd_frames_are_one_body() {
        let mut body = zstd::encode_all(&b"hello "[..], 3).unwrap();
        body.extend(zstd::encode_all(&b"world"[..], 3).unwrap());
        assert_eq!(&decompress("zstd", Bytes::from(body))[..], b"hello world");
    }

    #[test]
    fn bare_deflate_is_taken_where_zlib_is_said() {
        let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"points").unwrap();
        let body = encoder.finish().unwrap();
        assert_eq!(&decompress("deflate", Bytes::from(body))[..], b"points");
    }

    #[test]
    fn a_body_that_does_not_decode_comes_back_unchanged() {
        assert_eq!(&decompress("gzip", Bytes::from_static(b"plain"))[..], b"plain");
    }
}
