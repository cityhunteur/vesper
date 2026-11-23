use clap::Parser;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "vesper-proxy")]
struct Args {
    #[arg(long, default_value = "/tmp/vesper/docker.sock")]
    listen: String,
    #[arg(long, default_value = "vsock://3:2375")]
    upstream: String,
    #[arg(long, default_value = "/tmp/vesper/vsock")]
    vsock_socket_dir: String,
    #[arg(long, default_value_t = 5)]
    dial_timeout_seconds: u64,
    /// Octal permission bits for the listen socket (owner-only by default).
    #[arg(long, default_value = "600")]
    listen_mode: String,
    /// Fail at startup unless the upstream socket (unix path or vsock bridge) exists.
    #[arg(long)]
    validate_upstream: bool,
    /// Also publish containers' `-p` ports on the host loopback, forwarding through the vsock bridge to the
    /// System VM's port relay (GH #28).
    #[arg(long)]
    publish_ports: bool,
    /// Guest vsock port of `vesper-port-relay`.
    #[arg(long, default_value_t = 2400)]
    relay_port: u32,
    /// Status file listing the current port mappings and their state.
    #[arg(long, default_value = "run/vesper/ports.json")]
    ports_status: String,
    /// Concurrent relayed connections; a client beyond it is closed at once, with no bytes (TD-021).
    #[arg(long, default_value_t = 256)]
    max_connections: usize,
    /// Bound on the vsock bridge's `CONNECT` preface (TD-021). There is no idle timeout: Docker's attach, logs and
    /// events streams can stay quiet for hours (D-072).
    #[arg(long, default_value_t = 5)]
    handshake_timeout_seconds: u64,
    /// Entitlements plist read before starting, as `vesperd` does (TD-017); the proxy needs no key, because it
    /// sandboxes itself (ADR 0012).
    #[arg(long)]
    entitlements_file: Option<String>,
    /// Run without the seatbelt profile (ADR 0012), for diagnosis only; a warning says so on every start.
    #[arg(long)]
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
}

fn absolute(path: &str) -> std::path::PathBuf {
    vesper::sandbox::absolute(std::path::Path::new(path))
}

/// What the proxy may touch (ADR 0012): its socket's directory, its socket (it also talks to it, for the publisher),
/// the bridge socket, the ports status file, and loopback ports when it publishes them.
fn sandbox_rules(args: &Args) -> Result<Vec<vesper::sandbox::Rule>, String> {
    use vesper::sandbox::Rule;
    let listen = absolute(&args.listen);
    // Made before the profile, which allows writing in these directories but not making their parents; a directory
    // that is a symlink would let whoever made it choose what the profile grants, so it is refused (council S2).
    let mut dirs = vec![listen.parent().map(|d| d.to_path_buf())];
    if args.publish_ports {
        dirs.push(
            absolute(&args.ports_status)
                .parent()
                .map(|d| d.to_path_buf()),
        );
    }
    for dir in dirs.into_iter().flatten() {
        vesper::sandbox::prepare_dir(&dir)?;
    }
    let mut rules = vec![
        Rule::ListenUnix(listen.clone()),
        Rule::ConnectUnix(listen.clone()),
    ];
    if let Some(dir) = listen.parent() {
        rules.push(Rule::WriteTree(dir.to_path_buf()));
    }
    let target = vesper::proxy::parse_target(&args.upstream)?;
    if target.scheme == "tcp" {
        // Loopback only under the profile; a remote TCP upstream would widen what the proxy can reach.
        let addr: std::net::SocketAddr = target
            .address
            .parse()
            .map_err(|_| format!("tcp upstream {} must be an ip:port", target.address))?;
        if !addr.ip().is_loopback() {
            return Err(format!(
                "tcp upstream {addr} is not loopback; the sandbox allows loopback only (--no-sandbox to try it)"
            ));
        }
        rules.push(Rule::ConnectLoopbackTcp(addr.port()));
    } else if let Some(up) = vesper::proxy::upstream_socket_path(&target, &args.vsock_socket_dir)? {
        let up = absolute(&up);
        rules.push(Rule::ReadFile(up.clone()));
        rules.push(Rule::ConnectUnix(up));
    }
    if args.publish_ports {
        let status = absolute(&args.ports_status);
        if let Some(dir) = status.parent() {
            rules.push(Rule::WriteTree(dir.to_path_buf()));
        }
        rules.push(Rule::LoopbackInbound);
        rules.push(Rule::LoopbackUdpOutbound);
    }
    Ok(rules)
}

/// Applies the profile and proves it, or exits: a proxy never runs unconfined by accident (ADR 0012).
fn confine(args: &Args) {
    if args.no_sandbox {
        eprintln!(
            "vesper-proxy: WARNING: running without its sandbox profile (--no-sandbox), for diagnosis only"
        );
        return;
    }
    let checked = sandbox_rules(args).and_then(|rules| vesper::sandbox::confine(&rules));
    if let Err(err) = checked {
        eprintln!("vesper-proxy failed: {err}");
        std::process::exit(1);
    }
}

/// The --sandbox-probe test hook: one `PROBE <name> denied|ALLOWED` line per attempt.
fn probe(args: &Args) {
    let home = vesper::sandbox::canary().unwrap_or_default();
    let unix = args.probe_unix.as_deref().map(std::path::Path::new);
    for line in vesper::sandbox::probes(&home, args.probe_tcp, unix) {
        println!("{line}");
    }
}

fn main() {
    let args = Args::parse();
    // Read before the profile, which does not name the file.
    if let Err(err) = vesper::platform::validate_entitlements_file(
        args.entitlements_file.as_deref().map(std::path::Path::new),
        &[],
    ) {
        eprintln!("vesper-proxy failed: {err}");
        std::process::exit(1);
    }
    // The first thing after the arguments, before any thread, socket or log file (ADR 0012).
    confine(&args);
    if args.sandbox_probe {
        probe(&args);
        return;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|err| {
            eprintln!("vesper-proxy failed: tokio runtime: {err}");
            std::process::exit(1);
        });
    runtime.block_on(serve(args));
}

async fn serve(args: Args) {
    vesper::platform::init_logging();

    let listen_mode = u32::from_str_radix(&args.listen_mode, 8).unwrap_or_else(|_| {
        eprintln!(
            "vesper-proxy failed: --listen-mode must be octal, got {}",
            args.listen_mode
        );
        std::process::exit(2);
    });

    // Ctrl-C or SIGTERM (supervisors and `make docker-down`) stop the server.
    let shutdown = async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    };

    let publisher = if args.publish_ports {
        let bridge = vesper::proxy::parse_target(&args.upstream)
            .and_then(|t| vesper::proxy::upstream_socket_path(&t, &args.vsock_socket_dir))
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                eprintln!("vesper-proxy failed: --publish-ports needs a vsock upstream (the bridge socket is the gateway path)");
                std::process::exit(2);
            });
        let config = vesper::ports::PortsConfig {
            docker_socket: std::path::PathBuf::from(&args.listen),
            bridge_path: std::path::PathBuf::from(bridge),
            relay_port: args.relay_port,
            status_path: std::path::PathBuf::from(&args.ports_status),
            ..vesper::ports::PortsConfig::default()
        };
        Some(tokio::spawn(async move {
            // Give the ingress socket a moment to exist before the first Docker API call.
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Err(err) = vesper::ports::Publisher::new(config).run().await {
                eprintln!("ports: publisher stopped: {err}");
            }
        }))
    } else {
        None
    };

    let result = vesper::proxy::run_server(
        vesper::proxy::ServerConfig {
            listen_path: args.listen.clone(),
            upstream_raw: args.upstream.clone(),
            dial_timeout: Duration::from_secs(args.dial_timeout_seconds.max(1)),
            vsock_socket_dir: args.vsock_socket_dir.clone(),
            listen_mode,
            validate_upstream: args.validate_upstream,
            bounds: vesper::proxy::Bounds {
                handshake_timeout: Some(Duration::from_secs(args.handshake_timeout_seconds.max(1))),
                max_connections: Some(args.max_connections.max(1)),
                ..Default::default()
            },
            ..Default::default()
        },
        shutdown,
    )
    .await;

    if let Some(task) = publisher {
        task.abort();
    }
    if let Err(err) = result {
        eprintln!("vesper-proxy failed: {err}");
        std::process::exit(1);
    }
}
