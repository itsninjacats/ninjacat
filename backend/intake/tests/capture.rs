//! The capture lab through a host's router: what DEBUG=true writes for a request, and what it
//! leaves out.

mod db;
mod programs;

use std::fs;
use std::path::{Path, PathBuf};

use bytes::Bytes;
use ninjapaws_intake::capture;
use tako::router::Router;

const KEY: &str = "0123456789abcdef0123456789abcdef";

/// A directory of the test's own, removed when it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        Scratch(std::env::temp_dir().join(format!("ninjapaws-capture-{name}-{}", uuid::Uuid::new_v4().simple())))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Sends one request to `mount`'s host with captures on, with a key of `tenant`; returns the status.
async fn send(
    directory: &Path,
    clickhouse: clickhouse::Client,
    tenant: &str,
    mount: fn(&mut Router),
    request: http::Request<Vec<u8>>,
) -> u16 {
    let router = programs::router_for(mount, &clickhouse, KEY, tenant);
    let directory = directory.to_path_buf();
    router.middleware(move |req, next| capture::dump(directory.clone(), req, next));
    let (parts, body) = request.into_parts();
    router.dispatch(http::Request::from_parts(parts, Bytes::from(body).into())).await.status().as_u16()
}

/// The one capture file in `directory`, and its index.log.
fn only_capture(directory: &Path) -> (String, String, String) {
    let mut names: Vec<String> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "index.log")
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    let name = names.remove(0);
    let text = fs::read_to_string(directory.join(&name)).unwrap();
    let index = fs::read_to_string(directory.join("index.log")).unwrap();
    (name, text, index)
}

#[tokio::test]
async fn a_capture_is_a_file_and_an_index_line_with_keys_left_out() {
    let scratch = Scratch::new("series");
    let request = http::Request::builder()
        .method("POST")
        .uri("/api/v2/series")
        .header("Host", "api.ninjacat.local")
        .header("Dd-Api-Key", KEY)
        .header("User-Agent", "datadog-agent/7.60.0")
        .header("Content-Type", "application/json")
        .body(br#"{"series":[]}"#.to_vec())
        .unwrap();
    // No series, so nothing is inserted.
    assert_eq!(202, send(&scratch.0, db::unconnected(), "test", programs::api, request).await);

    let (name, text, index) = only_capture(&scratch.0);
    assert!(name.contains("_POST_api_v2_series_") && name.ends_with(".txt"), "{name}");
    assert!(text.contains("POST /api/v2/series HTTP/1.1"), "{text}");
    assert!(text.contains("Host: api.ninjacat.local"), "{text}");
    assert!(text.contains("# status:   202"), "{text}");
    assert!(text.contains("user-agent: datadog-agent/7.60.0"), "{text}");
    assert!(text.contains("\"series\": []"), "{text}");
    assert!(!text.contains("0123456789abcdef"), "{text}");
    assert!(text.contains("dd-api-key: <redacted>"), "{text}");
    assert!(index.contains(&format!("\tPOST\t/api/v2/series\t202\t13\t13\t-\t{name}\n")), "{index}");
}

#[tokio::test]
async fn a_key_in_the_query_or_the_path_is_left_out_and_the_body_is_decompressed() {
    let scratch = Scratch::new("input");
    let body = r#"[{"message":"hello"}]"#;
    let compressed = {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(body.as_bytes()).unwrap();
        encoder.finish().unwrap()
    };
    let raw_length = compressed.len();
    let request = http::Request::builder()
        .method("POST")
        .uri(format!("/v1/input/{KEY}?ddsource=a%20b&api_key={KEY}"))
        .header("Content-Encoding", "gzip")
        .header("Content-Type", "application/json")
        .body(compressed)
        .unwrap();
    // Without ClickHouse the insert fails and the capture is the same; with it, the capture is also
    // shown not to stand in the way of the write.
    let client = db::scratch_client().await;
    let tenant = db::tenant();
    let clickhouse = client.clone().unwrap_or_else(db::unconnected);
    let status = send(&scratch.0, clickhouse, &tenant, programs::logs, request).await;
    if let Some(client) = client {
        assert_eq!(status, 202);
        assert_eq!(db::tables(&client, &tenant).await, [("logs".to_string(), 1)].into());
    }

    let (name, text, index) = only_capture(&scratch.0);
    assert!(!name.contains(KEY) && !text.contains(KEY) && !index.contains(KEY), "{name}\n{text}\n{index}");
    assert!(text.contains("POST /v1/input/<key>?ddsource=a%20b&api_key=%3Credacted%3E HTTP/1.1"), "{text}");
    assert!(
        text.contains(&format!("# body:     {raw_length} B raw -> {} B decompressed (gzip)", body.len())),
        "{text}"
    );
    assert!(text.contains("\"message\": \"hello\""), "{text}");
    assert!(index.contains("\t/v1/input/<key>\t"), "{index}");
}

#[tokio::test]
async fn a_request_nothing_serves_is_captured_too() {
    let scratch = Scratch::new("unknown");
    let request = http::Request::builder().method("PUT").uri("/api/v9/nothing").body(b"abc".to_vec()).unwrap();
    assert_eq!(404, send(&scratch.0, db::unconnected(), "test", programs::api, request).await);

    let (name, text, _) = only_capture(&scratch.0);
    assert!(name.contains("_PUT_api_v9_nothing_"), "{name}");
    assert!(text.contains("# status:   404"), "{text}");
    assert!(text.ends_with("---------------- BODY ----------------\nabc\n"), "{text}");
}
