use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(name = "baselinectl")]
struct Args {
    #[arg(long)]
    mode: String,
    #[arg(long, default_value = "docs/specs/design-v0.baselines.lock.yaml")]
    lock: String,
    #[arg(long, default_value = "artifacts/parity-metadata.json")]
    metadata: String,
    #[arg(long, default_value = "")]
    backend: String,
    #[arg(long, default_value = "")]
    host_os: String,
    #[arg(long, default_value = "")]
    host_cpu: String,
}

#[derive(Serialize, Deserialize)]
struct MetadataEntry {
    key: String,
    value: String,
}

#[derive(Serialize, Deserialize)]
struct MetadataPayload {
    metadata: Vec<MetadataEntry>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    let lock = vesper::baseline::load(&args.lock)?;

    match args.mode.as_str() {
        "generate" => generate(&lock, &args),
        "validate" => validate(&lock, &args.metadata),
        other => Err(format!(
            "invalid mode {other:?} (expected generate|validate)"
        )),
    }
}

fn generate(lock: &vesper::baseline::Lock, args: &Args) -> Result<(), String> {
    let metadata =
        vesper::baseline::build_metadata(lock, &args.backend, &args.host_os, &args.host_cpu)?;

    let mut entries = Vec::with_capacity(metadata.len());
    for (key, value) in metadata {
        entries.push(MetadataEntry { key, value });
    }

    let payload = MetadataPayload { metadata: entries };

    if let Some(parent) = Path::new(&args.metadata).parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("create metadata directory {}: {err}", parent.display()))?;
    }

    let mut content = serde_json::to_string_pretty(&payload)
        .map_err(|err| format!("serialize metadata: {err}"))?;
    content.push('\n');
    fs::write(&args.metadata, content)
        .map_err(|err| format!("write metadata file {}: {err}", &args.metadata))?;

    println!("generated metadata at {}", args.metadata);
    Ok(())
}

fn validate(lock: &vesper::baseline::Lock, metadata_path: &str) -> Result<(), String> {
    let content = fs::read_to_string(metadata_path)
        .map_err(|err| format!("read metadata file {metadata_path}: {err}"))?;

    let payload: MetadataPayload =
        serde_json::from_str(&content).map_err(|err| format!("parse metadata JSON: {err}"))?;

    if payload.metadata.is_empty() {
        return Err("metadata payload is empty".to_string());
    }

    let mut metadata = BTreeMap::new();
    for entry in payload.metadata {
        metadata.insert(entry.key, entry.value);
    }

    vesper::baseline::validate_metadata(lock, &metadata)?;
    println!("metadata is valid: {metadata_path}");
    Ok(())
}
