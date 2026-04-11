use clap::Parser;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(name = "vesperd")]
struct Args {
    #[arg(long, default_value = "/tmp/vesperd.sock")]
    socket: String,
    #[arg(long, default_value = "docs/specs/design-v0.baselines.lock.yaml")]
    baseline_lock: String,
    #[arg(long, default_value = "third_party/firecracker/firecracker.yaml")]
    swagger: String,
    #[arg(
        long,
        default_value = "docs/generated/firecracker-api-surface-stub-matrix.json"
    )]
    surface_stub_matrix: String,
    #[arg(long)]
    entitlements_file: Option<String>,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("vesperd failed: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = Args::parse();

    vesper::platform::validate_entitlements_file(
        args.entitlements_file.as_deref().map(Path::new),
        &["com.apple.security.virtualization"],
    )?;

    let lock = vesper::baseline::load(&args.baseline_lock)?;
    let firecracker_revision = lock
        .baselines
        .get("firecracker")
        .map(|value| value.revision.clone())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "baseline lock missing firecracker revision".to_string())?;

    let routes = vesper::spec::load_routes(&args.swagger)?
        .into_iter()
        .map(|route| vesper::api::RouteSpec {
            method: route.method,
            path: route.path,
        })
        .collect::<Vec<_>>();
    let firecracker_version = vesper::spec::load_info_version(&args.swagger)?;
    let surface_stub_matrix =
        vesper::api_matrix::load_surface_stub_matrix(&args.surface_stub_matrix)?;

    let app_state = vesper::api::AppState::new_with_matrix(
        &routes,
        firecracker_revision,
        firecracker_version,
        &surface_stub_matrix,
    )?;
    let app = app_state.router();

    if let Some(parent) = Path::new(&args.socket).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| format!("create socket dir {}: {err}", parent.display()))?;
    }
    let _ = tokio::fs::remove_file(&args.socket).await;

    let listener = tokio::net::UnixListener::bind(&args.socket)
        .map_err(|err| format!("bind unix socket {}: {err}", &args.socket))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o660);
        std::fs::set_permissions(&args.socket, perms)
            .map_err(|err| format!("set socket permissions {}: {err}", &args.socket))?;
    }

    println!("vesperd listening on {}", &args.socket);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|err| format!("serve: {err}"))?;

    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("register SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
