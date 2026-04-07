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
    #[arg(long, default_value = "third_party/firecracker/firecracker.yaml")]
    swagger: String,
    #[arg(
        long,
        default_value = "docs/generated/firecracker-api-surface-stub-matrix.json"
    )]
    matrix: String,
    #[arg(
        long,
        default_value = "docs/specs/firecracker-api-baseline-captures.yaml"
    )]
    captures: String,
    #[arg(
        long,
        default_value = "docs/generated/firecracker-api-behavior-matrix.json"
    )]
    behavior_matrix: String,
    #[arg(
        long,
        default_value = "docs/specs/firecracker-api-behavior-captures.yaml"
    )]
    behavior_captures: String,
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
        "generate-api-matrix" => generate_api_matrix(&lock, &args),
        "validate-api-matrix" => validate_api_matrix(&lock, &args),
        "generate-api-behavior-matrix" => generate_api_behavior_matrix(&lock, &args),
        "validate-api-behavior-matrix" => validate_api_behavior_matrix(&lock, &args),
        other => Err(format!(
            "invalid mode {other:?} (expected generate|validate|generate-api-matrix|validate-api-matrix|generate-api-behavior-matrix|validate-api-behavior-matrix)"
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

fn generate_api_matrix(lock: &vesper::baseline::Lock, args: &Args) -> Result<(), String> {
    let firecracker_revision = firecracker_revision(lock)?;
    let firecracker_version = vesper::spec::load_info_version(&args.swagger)?;
    let routes = vesper::spec::load_routes(&args.swagger)?
        .into_iter()
        .map(|route| vesper::api::RouteSpec {
            method: route.method,
            path: route.path,
        })
        .collect::<Vec<_>>();
    let capture_registry = vesper::api_matrix::load_baseline_capture_registry(&args.captures)?;
    let matrix = vesper::api_matrix::generate_surface_stub_matrix_with_captures(
        &routes,
        &firecracker_revision,
        &firecracker_version,
        Some(&capture_registry),
        &args.captures,
    )?;

    vesper::api_matrix::write_surface_stub_matrix(&args.matrix, &matrix)?;
    println!("generated API surface stub matrix at {}", args.matrix);
    Ok(())
}

fn validate_api_matrix(lock: &vesper::baseline::Lock, args: &Args) -> Result<(), String> {
    let firecracker_revision = firecracker_revision(lock)?;
    let firecracker_version = vesper::spec::load_info_version(&args.swagger)?;
    let routes = vesper::spec::load_routes(&args.swagger)?
        .into_iter()
        .map(|route| vesper::api::RouteSpec {
            method: route.method,
            path: route.path,
        })
        .collect::<Vec<_>>();
    let capture_registry = vesper::api_matrix::load_baseline_capture_registry(&args.captures)?;
    let expected = vesper::api_matrix::generate_surface_stub_matrix_with_captures(
        &routes,
        &firecracker_revision,
        &firecracker_version,
        Some(&capture_registry),
        &args.captures,
    )?;
    let matrix = vesper::api_matrix::load_surface_stub_matrix(&args.matrix)?;

    vesper::api_matrix::validate_surface_stub_matrix(
        &matrix,
        &routes,
        &firecracker_revision,
        &firecracker_version,
    )?;
    if matrix != expected {
        return Err(format!(
            "API surface stub matrix {} is stale; regenerate it with baselinectl --mode generate-api-matrix",
            args.matrix
        ));
    }
    println!("API surface stub matrix is valid: {}", args.matrix);
    Ok(())
}

fn generate_api_behavior_matrix(lock: &vesper::baseline::Lock, args: &Args) -> Result<(), String> {
    let firecracker_revision = firecracker_revision(lock)?;
    let firecracker_version = vesper::spec::load_info_version(&args.swagger)?;
    let routes = vesper::spec::load_routes(&args.swagger)?
        .into_iter()
        .map(|route| vesper::api::RouteSpec {
            method: route.method,
            path: route.path,
        })
        .collect::<Vec<_>>();
    let capture_registry =
        vesper::api_behavior_matrix::load_api_behavior_capture_registry(&args.behavior_captures)?;
    let matrix = vesper::api_behavior_matrix::generate_api_behavior_matrix_with_captures(
        &routes,
        &firecracker_revision,
        &firecracker_version,
        Some(&capture_registry),
        &args.behavior_captures,
    )?;

    vesper::api_behavior_matrix::write_api_behavior_matrix(&args.behavior_matrix, &matrix)?;
    println!("generated API behavior matrix at {}", args.behavior_matrix);
    Ok(())
}

fn validate_api_behavior_matrix(lock: &vesper::baseline::Lock, args: &Args) -> Result<(), String> {
    let firecracker_revision = firecracker_revision(lock)?;
    let firecracker_version = vesper::spec::load_info_version(&args.swagger)?;
    let routes = vesper::spec::load_routes(&args.swagger)?
        .into_iter()
        .map(|route| vesper::api::RouteSpec {
            method: route.method,
            path: route.path,
        })
        .collect::<Vec<_>>();
    let capture_registry =
        vesper::api_behavior_matrix::load_api_behavior_capture_registry(&args.behavior_captures)?;
    let expected = vesper::api_behavior_matrix::generate_api_behavior_matrix_with_captures(
        &routes,
        &firecracker_revision,
        &firecracker_version,
        Some(&capture_registry),
        &args.behavior_captures,
    )?;
    let matrix = vesper::api_behavior_matrix::load_api_behavior_matrix(&args.behavior_matrix)?;

    vesper::api_behavior_matrix::validate_api_behavior_matrix(
        &matrix,
        &routes,
        &firecracker_revision,
        &firecracker_version,
    )?;
    if matrix != expected {
        return Err(format!(
            "API behavior matrix {} is stale; regenerate it with baselinectl --mode generate-api-behavior-matrix",
            args.behavior_matrix
        ));
    }
    println!("API behavior matrix is valid: {}", args.behavior_matrix);
    Ok(())
}

fn firecracker_revision(lock: &vesper::baseline::Lock) -> Result<String, String> {
    lock.baselines
        .get("firecracker")
        .map(|reference| reference.revision.clone())
        .filter(|revision| !revision.trim().is_empty())
        .ok_or_else(|| "baseline lock missing firecracker revision".to_string())
}
