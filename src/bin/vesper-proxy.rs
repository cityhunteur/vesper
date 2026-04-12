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
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    let result = vesper::proxy::run_server(
        vesper::proxy::ServerConfig {
            listen_path: args.listen.clone(),
            upstream_raw: args.upstream.clone(),
            dial_timeout: Duration::from_secs(args.dial_timeout_seconds.max(1)),
            vsock_socket_dir: args.vsock_socket_dir.clone(),
        },
        shutdown,
    )
    .await;

    if let Err(err) = result {
        eprintln!("vesper-proxy failed: {err}");
        std::process::exit(1);
    }
}
