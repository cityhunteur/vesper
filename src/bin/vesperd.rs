use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

#[path = "vesperd/stage.rs"]
mod stage;
#[path = "vesperd/systemvm.rs"]
mod systemvm;

/// `--network` for `--backend systemvm` (ADR 0008).
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Network {
    /// Taps only, with no uplink (CE-0010).
    None,
    /// A veth to the System VM, NATed to public IPv4 only, with DNS through each tap gateway.
    Nat,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Backend {
    /// The in-memory model of the API: a development and test surface that starts no microVM (ADR 0007 item 9).
    Model,
    /// A byte relay to one jailed Firecracker in the System VM (ADR 0007); the default (D-063).
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
    /// `STORE NAME bytes=N links=L` per file, then `FREE avail=N reserve=N store=N quota=N` (bytes: free on the data disk, the free-space
    /// reserve, the store's size and its quota, TD-053).
    List,
    /// Remove one store file; a microVM that has it linked keeps its own link.
    Rm { name: String },
}

#[derive(Parser, Debug)]
#[command(name = "vesperd")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    /// The backend; default `systemvm` (D-063). `model` answers from memory and starts no microVM.
    #[arg(long, value_enum)]
    backend: Option<Backend>,
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
    /// Create a tap device NAME for the microVM before Firecracker starts (`--backend systemvm`; repeatable, at most
    /// 8; 1 to 15 letters, digits, _ and -, not starting with -, not lo): it lives in the microVM's own network
    /// namespace and is owned by the jailer's uid, so `PUT /network-interfaces` with `host_dev_name` NAME succeeds
    /// (CE-0010). With `--network nat`, NAME:ADDR/PREFIX also gives the tap the gateway address its guest routes and
    /// resolves names through (RFC 1918, prefix length 16 to 30; ADR 0008).
    #[arg(long = "tap", value_name = "NAME[:ADDR/PREFIX]")]
    tap: Vec<String>,
    /// The microVM's network (`--backend systemvm`): `none` (the default; taps have no uplink) or `nat` (public IPv4
    /// only, over TCP, UDP and ICMP echo, plus DNS through the tap gateway; nothing on the System VM, the Mac, the
    /// LAN, containers or other microVMs; ADR 0008).
    #[arg(long, value_enum)]
    network: Option<Network>,
    /// System VM launch config that names the bridge socket and agent port (`--backend systemvm`, `stage`, `store`).
    #[arg(
        long,
        global = true,
        default_value = "build/systemvm/launch-config.json"
    )]
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
    /// Entitlements plist read before starting (TD-017); `vesperd` needs no key, because it sandboxes itself
    /// (ADR 0012).
    #[arg(long)]
    entitlements_file: Option<String>,
    /// A directory the model backend may open host paths in (`--backend model`; repeatable). Default: the working
    /// directory and the per-user temporary directory macOS assigns. A request that names a path outside them gets the error a path that cannot be
    /// opened gets (ADR 0012, CE-0013).
    #[arg(long = "host-root", value_name = "DIR")]
    host_root: Vec<String>,
    /// Run without the seatbelt profile (ADR 0012), for diagnosis only; a warning says so on every start.
    #[arg(long, global = true)]
    no_sandbox: bool,
    /// Test hook (ADR 0012): apply the profile, try what it must refuse, print one `PROBE` line each, and exit.
    #[arg(long, hide = true)]
    sandbox_probe: bool,
    /// With --sandbox-probe: a loopback TCP port to try to connect to.
    #[arg(long, hide = true)]
    probe_tcp: Option<u16>,
    /// With --sandbox-probe: a Unix socket the profile does not name.
    #[arg(long, hide = true)]
    probe_unix: Option<String>,
    /// With --sandbox-probe: a file the model backend must not open (outside every --host-root).
    #[arg(long, hide = true)]
    probe_file: Option<String>,
}

fn main() {
    let args = Args::parse();
    // Read before the profile, which does not name the file.
    if let Err(err) = vesper::platform::validate_entitlements_file(
        args.entitlements_file.as_deref().map(Path::new),
        &[],
    ) {
        eprintln!("vesperd failed: {err}");
        std::process::exit(1);
    }
    // The first thing after the arguments, before any thread, socket or log file (ADR 0012).
    let socket = confine(&args);
    if args.sandbox_probe {
        probe(&args);
        return;
    }
    vesper::platform::init_logging();
    if let Some(command) = &args.command {
        std::process::exit(stage::run(&args, command));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| {
            eprintln!("vesperd failed: tokio runtime: {err}");
            std::process::exit(1);
        });
    if let Err(err) = runtime.block_on(run(args, socket)) {
        eprintln!("vesperd failed: {err}");
        std::process::exit(1);
    }
}

fn check_model_flags(args: &Args) -> Result<(), String> {
    if args.id.is_some()
        || args.memory_max.is_some()
        || args.bridge.is_some()
        || args.agent_port.is_some()
        || !args.stage.is_empty()
        || !args.tap.is_empty()
        || args.network.is_some()
    {
        return Err("--id, --memory-max, --stage, --tap, --network, --bridge and --agent-port do not apply to --backend model".to_string());
    }
    Ok(())
}

fn is_model(args: &Args) -> bool {
    args.command.is_none() && args.backend == Some(Backend::Model)
}

/// The directories the model backend may open host paths in: `--host-root`, else the working directory and the
/// user's temporary directory (on macOS the one the system assigns, not `$TMPDIR`, so no environment value widens
/// the profile). A root that covers a home directory is refused, which keeps the self-check meaningful.
fn host_roots(args: &Args) -> Result<Vec<PathBuf>, String> {
    let explicit = !args.host_root.is_empty();
    let roots: Vec<PathBuf> = if explicit {
        args.host_root
            .iter()
            .map(|r| vesper::sandbox::absolute(Path::new(r)))
            .collect()
    } else {
        let cwd = std::env::current_dir()
            .map_err(|e| format!("the working directory: {e}; pass --host-root DIR"))?;
        std::iter::once(cwd)
            .chain(vesper::sandbox::user_temp_dir())
            .collect()
    };
    // Only macOS has a profile, and so a self-check to keep meaningful.
    for root in roots.iter().filter(|_| cfg!(target_os = "macos")) {
        let real = vesper::sandbox::canonical(root)?;
        for home in vesper::sandbox::homes() {
            if vesper::sandbox::canonical(&home).is_ok_and(|h| h.starts_with(&real)) {
                return Err(if explicit {
                    format!(
                        "--host-root {} covers the home directory {}; name a directory inside it",
                        root.display(),
                        home.display()
                    )
                } else {
                    format!(
                        "the working directory {} covers the home directory {}, so it cannot be the model backend's default host root; pass --host-root DIR",
                        root.display(),
                        home.display()
                    )
                });
            }
        }
    }
    Ok(roots)
}

/// What `vesperd` may touch (ADR 0012), and its API socket (none for `stage` and `store`). Every mode may read the
/// files it names; a server may listen on its socket and take `<socket>.lock`; the System VM backend and the
/// subcommands may connect to the bridge socket; the model backend may read and write under its host roots.
/// Nothing may start a process or use the network.
fn sandbox_rules(args: &Args) -> Result<(Vec<vesper::sandbox::Rule>, Option<PathBuf>), String> {
    use vesper::sandbox::{Rule, absolute};
    if !args.host_root.is_empty() && !is_model(args) {
        return Err("--host-root applies to --backend model only".to_string());
    }
    // The command line is checked first, so a bad one makes no directory.
    if args.command.is_none() {
        if is_model(args) {
            check_model_flags(args)?;
        } else {
            systemvm::Options::from_args(args)?;
        }
    }
    let mut rules = Vec::new();
    let socket = if args.command.is_none() {
        let socket = absolute(&socket_path(args)?);
        // claim_socket makes a missing directory; under the profile it could not, so it is made here.
        if let Some(dir) = socket.parent() {
            vesper::sandbox::prepare_dir(dir)?;
            rules.push(Rule::ReadMetadata(dir.to_path_buf()));
        }
        let mut lock = socket.clone().into_os_string();
        lock.push(".lock");
        rules.push(Rule::WriteFile(PathBuf::from(lock)));
        rules.push(Rule::ListenUnix(socket.clone()));
        // claim_socket connects to an old socket to see whether a server still answers on it.
        rules.push(Rule::ConnectUnix(socket.clone()));
        Some(socket)
    } else {
        None
    };
    if is_model(args) {
        for file in [
            &args.baseline_lock,
            &args.swagger,
            &args.surface_stub_matrix,
        ] {
            rules.push(Rule::ReadFile(absolute(Path::new(file))));
        }
        for root in host_roots(args)? {
            rules.push(Rule::WriteTree(root));
        }
    } else {
        rules.push(Rule::ReadFile(absolute(Path::new(&args.launch_config))));
        if args.command.is_none() {
            rules.push(Rule::ReadFile(absolute(Path::new(&args.swagger))));
        }
        // An error here (no launch config) is reported by the mode itself, after the profile.
        if let Ok((bridge, _)) = systemvm::bridge_and_port(args) {
            let bridge = absolute(&bridge);
            rules.push(Rule::ReadFile(bridge.clone()));
            rules.push(Rule::ConnectUnix(bridge));
        }
    }
    Ok((rules, socket))
}

/// Applies the profile and proves it, or exits: `vesperd` never runs unconfined by accident (ADR 0012). Returns
/// the API socket path, chosen once for both the profile and the server.
fn confine(args: &Args) -> Option<PathBuf> {
    if args.no_sandbox {
        eprintln!(
            "vesperd: WARNING: running without its sandbox profile (--no-sandbox), for diagnosis only"
        );
        return args
            .command
            .is_none()
            .then(|| socket_path(args).ok())
            .flatten();
    }
    match sandbox_rules(args).and_then(|(rules, socket)| {
        vesper::sandbox::confine(&rules)?;
        Ok(socket)
    }) {
        Ok(socket) => socket,
        Err(err) => {
            eprintln!("vesperd failed: {err}");
            std::process::exit(1);
        }
    }
}

/// The --sandbox-probe test hook: one `PROBE <name> denied|ALLOWED` line per attempt. With `--probe-file`, also
/// opens that file as the model backend opens a drive.
fn probe(args: &Args) {
    let home = vesper::sandbox::canary().unwrap_or_default();
    let unix = args.probe_unix.as_deref().map(Path::new);
    let mut lines = vesper::sandbox::probes(&home, args.probe_tcp, unix);
    if let Some(file) = &args.probe_file {
        let opened = std::fs::OpenOptions::new().read(true).open(file);
        lines.push(format!(
            "PROBE open_outside_host_root {}",
            if opened.is_ok() { "ALLOWED" } else { "denied" }
        ));
    }
    for line in lines {
        println!("{line}");
    }
}

/// How long requests in flight may run after the stop signal (TD-024).
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

async fn run(args: Args, socket: Option<PathBuf>) -> Result<(), String> {
    let socket = match socket {
        Some(socket) => socket,
        None => socket_path(&args)?,
    };
    // No fallback (ADR 0007 item 9): without --backend model, a failed start is an error, never the model.
    if args.backend.unwrap_or(Backend::Systemvm) == Backend::Systemvm {
        let options = systemvm::Options::from_args(&args)?;
        return systemvm::run(options, &socket).await;
    }
    check_model_flags(&args)?;

    let lock = vesper::baseline::load(&args.baseline_lock)?;
    let firecracker_revision = lock
        .baselines
        .get("firecracker")
        .map(|value| value.revision.clone())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "baseline lock missing firecracker revision".to_string())?;

    let routes = vesper::spec::load_routes(&args.swagger)?;
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

    // Held for the life of the process: a second vesperd on the same path waits for nothing and fails.
    let _lock = vesper::proxy::claim_socket(&socket)?;
    let listener = bind_owner_only(&socket)?;

    println!("vesperd listening on {}", socket.display());

    // A request that blocks (a host path that is a FIFO, TD-024) would keep the graceful shutdown waiting for ever:
    // after the signal, requests in flight get SHUTDOWN_GRACE, then the process exits.
    let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel::<()>();
    let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
        shutdown_signal().await;
        let _ = stopped_tx.send(());
    });
    tokio::select! {
        result = serve => result.map_err(|err| format!("serve: {err}"))?,
        _ = async {
            if stopped_rx.await.is_ok() {
                tokio::time::sleep(SHUTDOWN_GRACE).await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            tracing::warn!(grace_s = SHUTDOWN_GRACE.as_secs(), "requests still in flight after the stop signal; exiting");
            std::process::exit(0);
        }
    }

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
    let base = [
        ("XDG_RUNTIME_DIR", "vesper"),
        ("TMPDIR", "vesper"),
        ("HOME", ".vesper/run"),
    ]
    .iter()
    .find_map(|(var, sub)| {
        std::env::var_os(var)
            .filter(|v| !v.is_empty())
            .map(|v| PathBuf::from(v).join(sub))
    })
    .ok_or_else(|| {
        "no XDG_RUNTIME_DIR, TMPDIR or HOME to place the default socket in; pass --socket"
            .to_string()
    })?;
    if !base.exists() {
        std::fs::create_dir_all(&base)
            .map_err(|err| format!("create socket dir {}: {err}", base.display()))?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("set socket dir permissions {}: {err}", base.display()))?;
    }
    Ok(base.join("vesperd.sock"))
}

/// Binds `socket` (after `claim_socket`) and makes it owner-only before it listens. A socket that does not listen
/// yet refuses every connect, so no client reaches the server through a socket whose mode is still the one the
/// umask gave it. (Bound and listening at once and then changed, the socket took connects for a moment with that
/// mode: with a umask that leaves the socket writable by others, another user could connect then, and a test that
/// read the mode as soon as a connect worked saw 0755 once in CI.)
fn bind_owner_only(socket: &Path) -> Result<tokio::net::UnixListener, String> {
    use std::os::unix::fs::PermissionsExt;
    let unbound = tokio::net::UnixSocket::new_stream()
        .map_err(|err| format!("make a unix socket for {}: {err}", socket.display()))?;
    unbound
        .bind(socket)
        .map_err(|err| format!("bind unix socket {}: {err}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|err| format!("set socket permissions {}: {err}", socket.display()))?;
    unbound
        .listen(1024)
        .map_err(|err| format!("listen on unix socket {}: {err}", socket.display()))
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
