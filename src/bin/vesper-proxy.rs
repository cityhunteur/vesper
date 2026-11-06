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
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let listen_mode = u32::from_str_radix(&args.listen_mode, 8).unwrap_or_else(|_| {
        eprintln!("vesper-proxy failed: --listen-mode must be octal, got {}", args.listen_mode);
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

    let result = vesper::proxy::run_server(
        vesper::proxy::ServerConfig {
            listen_path: args.listen.clone(),
            upstream_raw: args.upstream.clone(),
            dial_timeout: Duration::from_secs(args.dial_timeout_seconds.max(1)),
            vsock_socket_dir: args.vsock_socket_dir.clone(),
            listen_mode,
            validate_upstream: args.validate_upstream,
        },
        shutdown,
    )
    .await;

    if let Err(err) = result {
        eprintln!("vesper-proxy failed: {err}");
        std::process::exit(1);
    }
}
