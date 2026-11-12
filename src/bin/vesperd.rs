use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "vesperd")]
struct Args {
    /// API socket. Default: `vesperd.sock` in an owner-only runtime directory, the first of
    /// `$XDG_RUNTIME_DIR/vesper`, `$TMPDIR/vesper` and `$HOME/.vesper/run` (TD-050).
    #[arg(long)]
    socket: Option<String>,
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

    let socket = match args.socket.as_deref() {
        Some(path) => PathBuf::from(path),
        None => default_socket()?,
    };
    // Held for the life of the process: a second vesperd on the same path waits for nothing and fails.
    let _lock = claim_socket(&socket)?;
    let listener = tokio::net::UnixListener::bind(&socket)
        .map_err(|err| format!("bind unix socket {}: {err}", socket.display()))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
            .map_err(|err| format!("set socket permissions {}: {err}", socket.display()))?;
    }

    println!("vesperd listening on {}", socket.display());

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|err| format!("serve: {err}"))?;

    Ok(())
}

/// The default socket path: `vesperd.sock` in the first available runtime directory (TD-050).
fn default_socket() -> Result<PathBuf, String> {
    let base = [("XDG_RUNTIME_DIR", "vesper"), ("TMPDIR", "vesper"), ("HOME", ".vesper/run")]
        .iter()
        .find_map(|(var, sub)| {
            std::env::var_os(var).filter(|v| !v.is_empty()).map(|v| PathBuf::from(v).join(sub))
        })
        .ok_or_else(|| "no XDG_RUNTIME_DIR, TMPDIR or HOME to place the default socket in; pass --socket".to_string())?;
    if !base.exists() {
        std::fs::create_dir_all(&base).map_err(|err| format!("create socket dir {}: {err}", base.display()))?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("set socket dir permissions {}: {err}", base.display()))?;
    }
    Ok(base.join("vesperd.sock"))
}

/// Checks the socket's directory, takes `<socket>.lock`, and removes a stale socket. Refuses when the directory is
/// writable by group or others, when another vesperd answers on the path, or when the path is not a socket.
fn claim_socket(socket: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let parent = socket.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if !parent.exists() {
        std::fs::create_dir_all(parent).map_err(|err| format!("create socket dir {}: {err}", parent.display()))?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("set socket dir permissions {}: {err}", parent.display()))?;
    }
    let mode = std::fs::metadata(parent)
        .map_err(|err| format!("socket dir {}: {err}", parent.display()))?
        .permissions()
        .mode();
    if mode & 0o022 != 0 {
        return Err(format!(
            "socket dir {} is writable by group or others (mode {:o}); use a private directory",
            parent.display(),
            mode & 0o7777
        ));
    }
    let mut lock_path = socket.as_os_str().to_owned();
    lock_path.push(".lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|err| format!("open {}: {err}", Path::new(&lock_path).display()))?;
    lock.try_lock()
        .map_err(|_| format!("another vesperd holds {}", Path::new(&lock_path).display()))?;
    if let Ok(meta) = std::fs::symlink_metadata(socket) {
        if !meta.file_type().is_socket() {
            return Err(format!("{} exists and is not a socket; refusing to remove it", socket.display()));
        }
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Err(format!("another process is serving {}", socket.display()));
        }
        std::fs::remove_file(socket).map_err(|err| format!("remove stale socket {}: {err}", socket.display()))?;
    }
    Ok(lock)
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
