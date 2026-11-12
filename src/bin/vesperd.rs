use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

#[path = "vesperd/stage.rs"]
mod stage;
#[path = "vesperd/systemvm.rs"]
mod systemvm;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Backend {
    /// The in-memory state machine (the default; ADR 0007 keeps it until the relay is proven).
    Model,
    /// A byte relay to one jailed Firecracker in the System VM (ADR 0007).
    Systemvm,
}

/// `vesperd stage` and `vesperd store` (ADR 0007 section 1): files for `--backend systemvm` microVMs.
#[derive(Subcommand, Debug)]
enum Command {
    /// Copy SRC to DEST while a microVM runs. SRC: share:/PATH (inside a configured share), store:NAME or
    /// jail:/PATH (in the microVM's jail). DEST: /PATH in the jail or store:NAME. Never links, never overwrites.
    Stage {
        /// The microVM whose jail SRC or DEST names (needed when either is in the jail).
        #[arg(long)]
        id: Option<String>,
        src: String,
        dest: String,
    },
    /// List or remove files in the snapshot store shared by the macOS user's vesperd microVMs.
    Store {
        #[command(subcommand)]
        action: StoreAction,
    },
}

#[derive(Subcommand, Debug)]
enum StoreAction {
    /// `STORE NAME bytes=N links=L` per file, then `FREE avail=N reserve=N` (bytes on the data disk).
    List,
    /// Remove one store file; a microVM that has it linked keeps its own link.
    Rm { name: String },
}

#[derive(Parser, Debug)]
#[command(name = "vesperd")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(long, value_enum, default_value_t = Backend::Model)]
    backend: Backend,
    /// The microVM id (`--backend systemvm`): 1 to 64 letters, digits and hyphens, as Firecracker's `--id`.
    #[arg(long)]
    id: Option<String>,
    /// `memory.max` in bytes for the microVM's cgroup (`--backend systemvm`); default: the agent's 640 MiB.
    #[arg(long)]
    memory_max: Option<u64>,
    /// Stage a file into the fresh jail before Firecracker starts (`--backend systemvm`; repeatable, at most 16):
    /// share:/PATH:/DEST or store:NAME:/DEST. Store files named *.mem or *.vmstate are hard-linked, root-owned
    /// and read-only; everything else is copied.
    #[arg(long = "stage", value_name = "SRC:DEST")]
    stage: Vec<String>,
    /// System VM launch config that names the bridge socket and agent port (`--backend systemvm`, `stage`, `store`).
    #[arg(long, global = true, default_value = "build/systemvm/launch-config.json")]
    launch_config: String,
    /// Bridge socket, overriding the launch config (`--backend systemvm`, `stage`, `store`).
    #[arg(long, global = true)]
    bridge: Option<String>,
    /// Agent vsock port, overriding the launch config (`--backend systemvm`, `stage`, `store`).
    #[arg(long, global = true)]
    agent_port: Option<u32>,
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
    let args = Args::parse();
    if let Some(command) = &args.command {
        std::process::exit(stage::run(&args, command));
    }
    if let Err(err) = run(args).await {
        eprintln!("vesperd failed: {err}");
        std::process::exit(1);
    }
}

async fn run(args: Args) -> Result<(), String> {

    vesper::platform::validate_entitlements_file(
        args.entitlements_file.as_deref().map(Path::new),
        &["com.apple.security.virtualization"],
    )?;

    if args.backend == Backend::Systemvm {
        let options = systemvm::Options::from_args(&args)?;
        return systemvm::run(options, &socket_path(&args)?).await;
    }
    if args.id.is_some() || args.memory_max.is_some() || args.bridge.is_some() || args.agent_port.is_some() || !args.stage.is_empty() {
        return Err("--id, --memory-max, --stage, --bridge and --agent-port apply to --backend systemvm".to_string());
    }

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

    let socket = socket_path(&args)?;
    // Held for the life of the process: a second vesperd on the same path waits for nothing and fails.
    let _lock = vesper::proxy::claim_socket(&socket)?;
    let listener = bind_owner_only(&socket)?;

    println!("vesperd listening on {}", socket.display());

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|err| format!("serve: {err}"))?;

    Ok(())
}

fn socket_path(args: &Args) -> Result<PathBuf, String> {
    match args.socket.as_deref() {
        Some(path) => Ok(PathBuf::from(path)),
        None => default_socket(),
    }
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

/// Binds `socket` (after `claim_socket`) and makes it owner-only.
fn bind_owner_only(socket: &Path) -> Result<tokio::net::UnixListener, String> {
    use std::os::unix::fs::PermissionsExt;
    let listener = tokio::net::UnixListener::bind(socket)
        .map_err(|err| format!("bind unix socket {}: {err}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("set socket permissions {}: {err}", socket.display()))?;
    Ok(listener)
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
