//! Regenerates `proto/src/generated/` from Datadog's `.proto` files: `cargo run -p protogen`.

use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    // proto/protos/README.md says where each came from.
    let root = backend.join("proto/protos");
    let out = backend.join("proto/src/generated");

    let includes = [
        root.join("agent-payload"),
        root.join("agent-payload/proto/deps"),
        root.join("datadog-agent"),
        root.join("sketches-go"),
        root.join("pprof"),
    ];

    let mut files = Vec::new();
    for dir in ["agent-payload/proto", "datadog-agent", "sketches-go", "pprof"] {
        protos_in(&root.join(dir), &mut files)?;
    }
    // The vendored deps come in through the imports that need them, so none
    // ends up in the set under two names.
    files.retain(|f| !f.starts_with(root.join("agent-payload/proto/deps")));

    let fds = protox::compile(&files, &includes)?;
    let descriptors = prost::Message::encode_to_vec(&fds);
    let mut packages: Vec<String> =
        fds.file.iter().map(|f| format!(".{}", f.package())).filter(|p| p != ".google.protobuf").collect();
    packages.sort();
    packages.dedup();

    // A package dropped from the protos must not live on as a stale file.
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out)?;
    // The well-known types from pbjson-types, which have the JSON mapping prost-types lacks.
    prost_build::Config::new()
        .out_dir(&out)
        .include_file("mod.rs")
        .compile_well_known_types()
        .extern_path(".google.protobuf", "::pbjson_types")
        .compile_fds(fds)?;

    // proto3's JSON mapping, as protobuf's own JSON parsers and printers have it: for the bodies
    // sent as JSON, and for messages stored as JSON text. Unknown fields are passed over, as the
    // F# server's parser does.
    pbjson_build::Builder::new()
        .register_descriptors(&descriptors)?
        .out_dir(&out)
        .ignore_unknown_fields()
        .build(&packages)?;
    // pbjson writes `<package>.serde.rs` beside each `<package>.rs`; prost's mod.rs includes only
    // the second, so the second includes the first. prost escapes a keyword in the name
    // (`protoc.r#gen.jsonschema.rs`), pbjson does not.
    for entry in fs::read_dir(&out)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        let Some(package) = name.strip_suffix(".rs") else { continue };
        if name == "mod.rs" || package.ends_with(".serde") {
            continue;
        }
        let serde = format!("{}.serde.rs", package.replace("r#", ""));
        if out.join(&serde).exists() {
            let mut code = fs::read_to_string(&path)?;
            code.push_str(&format!("include!(\"{serde}\");\n"));
            fs::write(&path, code)?;
        }
    }

    println!("generated {}", out.canonicalize()?.display());
    Ok(())
}

fn protos_in(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            protos_in(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "proto") {
            out.push(path);
        }
    }
    Ok(())
}
