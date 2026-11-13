//! `vesperd --backend systemvm` (ADR 0007 §2 to §5, slices 4 and 5): a byte relay to one jailed Firecracker in the
//! System VM.
//!
//! At start `vesperd` claims its socket (`<socket>.lock`, live-socket refusal), checks over `DATA status` that the
//! System VM's data disk is in use (it holds the jails and the snapshot store), sends `LAUNCHAPI <id> [ceiling]
//! [stage=SRC:DEST]...` to the agent through the bridge (one token per `--stage`, staged into the fresh jail before
//! Firecracker starts) and checks that `READY` names the pinned Firecracker version. That connection is the
//! microVM's lease for the life of the process: when the agent closes it (Firecracker exited, the System VM went
//! away), `vesperd` closes its listener, removes its socket and exits non-zero; on SIGTERM or SIGINT it closes the
//! lease, waits for the agent to confirm the teardown, and exits 0. Each client connection gets its own bridge
//! connection with the preface `FCAPI <id>` / `OK`, and then `proxy::serve` relays bytes under the bounds below.
//! `vesperd`'s own failures are refused or closed connections with no body; every HTTP byte comes from Firecracker.
//!
//! The values and the reason for each are in the ExecPlan
//! (`docs/exec-plans/completed/2026-11-12-GH156-vesperd-backend-a.md`, Values); compat exception CE-0007 records the
//! ones a client can observe.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::signal::unix::{SignalKind, signal};
use vesper::control::{
    AGENT_PROTOCOL_VERSION, DEFAULT_AGENT_PORT, MAX_STAGES, MAX_TAPS, Ready, StageSpec, data_disk_in_use, parse_ready,
    valid_microvm_id, valid_tap_name,
};
use vesper::proxy::{self, Bounds, Preface, ServerConfig, Target};

/// Dial of the bridge socket.
const DIAL_TIMEOUT: Duration = Duration::from_secs(5);
/// The `CONNECT` and preface writes, and the greeting, `OK` and `ERR` reads.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// `LAUNCHAPI` until `READY` or `ERR`.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// Added to `READY_TIMEOUT` when `--stage` copies files into the jail before Firecracker starts.
const STAGE_ALLOWANCE: Duration = Duration::from_secs(120);
/// Lines in a `DATA status` reply.
const MAX_DATA_LINES: usize = 4;
/// Longest greeting, `OK`, `READY` or `ERR` line.
const MAX_LINE: usize = 1024;
/// Bytes one relay read may fill.
const READ_BUFFER: usize = 8 * 1024;
/// A relayed connection with no request waiting is closed after this long without a byte.
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
/// A relayed connection whose request has had no reply byte for this long is closed.
const REQUEST_CEILING: Duration = Duration::from_secs(120);
/// Concurrent relayed connections; Firecracker itself accepts 10 and answers the next ones with its own 503.
const MAX_CONNECTIONS: usize = 16;
/// How long `ERR busy <id>` is retried: one reaper interval (10 s) plus a teardown (3 s grace and removal).
const REUSE_WAIT: Duration = Duration::from_secs(15);
const REUSE_RETRY: Duration = Duration::from_secs(1);
/// After closing the lease, how long to wait for the agent to tear the microVM down and close its side.
const STOP_WAIT: Duration = Duration::from_secs(20);

pub struct Options {
    id: String,
    memory_max: Option<u64>,
    stages: Vec<StageSpec>,
    taps: Vec<String>,
    bridge: PathBuf,
    port: u32,
    swagger: String,
}

/// The bridge socket and agent port: `--bridge` and `--agent-port`, else the launch config's.
pub fn bridge_and_port(args: &super::Args) -> Result<(PathBuf, u32), String> {
    let (cfg_bridge, cfg_port) = match (&args.bridge, std::fs::read_to_string(&args.launch_config)) {
        (Some(_), _) => (None, None),
        (None, Ok(text)) => {
            let cfg: vesper::systemvm::LaunchConfig = serde_json::from_str(&text)
                .map_err(|err| format!("parse launch config {}: {err}", args.launch_config))?;
            (cfg.vsock_bridge_path, cfg.agent_port)
        }
        (None, Err(err)) => {
            return Err(format!(
                "read launch config {}: {err} (pass --bridge or run make systemvm-launch-config)",
                args.launch_config
            ));
        }
    };
    let bridge = args
        .bridge
        .clone()
        .or(cfg_bridge)
        .ok_or("no bridge socket: the launch config has no vsock_bridge_path and --bridge was not given")?;
    Ok((PathBuf::from(bridge), args.agent_port.or(cfg_port).unwrap_or(DEFAULT_AGENT_PORT)))
}

impl Options {
    pub fn from_args(args: &super::Args) -> Result<Options, String> {
        let id = args.id.clone().ok_or("--backend systemvm needs --id")?;
        if !valid_microvm_id(&id) {
            return Err(format!("--id {id:?}: use 1 to 64 letters, digits and hyphens"));
        }
        if args.stage.len() > MAX_STAGES {
            return Err(format!("at most {MAX_STAGES} --stage flags"));
        }
        let stages = args.stage.iter().map(|s| StageSpec::parse(s)).collect::<Result<Vec<_>, _>>()?;
        let taps = check_taps(&args.tap)?;
        let (bridge, port) = bridge_and_port(args)?;
        Ok(Options { id, memory_max: args.memory_max, stages, taps, bridge, port, swagger: args.swagger.clone() })
    }
}

/// The relay's `ServerConfig`: the bridge `<dir>/<cid>.sock` as a vsock target, the `FCAPI` preface and the bounds.
fn relay_config(opts: &Options, socket: &Path) -> Result<ServerConfig, String> {
    let cid = opts
        .bridge
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.parse::<u32>().ok())
        .filter(|_| opts.bridge.extension().is_some_and(|ext| ext == "sock"))
        .ok_or_else(|| format!("bridge socket {} is not named <cid>.sock", opts.bridge.display()))?;
    let dir = opts.bridge.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    Ok(ServerConfig {
        listen_path: socket.to_string_lossy().into_owned(),
        upstream_raw: format!("vsock://{cid}:{}", opts.port),
        dial_timeout: DIAL_TIMEOUT,
        vsock_socket_dir: dir.to_string_lossy().into_owned(),
        listen_mode: 0o600,
        validate_upstream: false,
        preface: Some(Preface {
            greeting_prefix: greeting_prefix(),
            request: format!("FCAPI {}", opts.id),
            accept: "OK".to_string(),
        }),
        bounds: Bounds {
            handshake_timeout: Some(HANDSHAKE_TIMEOUT),
            max_line: MAX_LINE,
            read_buffer: READ_BUFFER,
            idle_timeout: Some(IDLE_TIMEOUT),
            request_ceiling: Some(REQUEST_CEILING),
            max_connections: Some(MAX_CONNECTIONS),
        },
    })
}

fn greeting_prefix() -> String {
    format!("VESPER-AGENT {AGENT_PROTOCOL_VERSION} ")
}

/// `--tap` names: at most `MAX_TAPS`, each a valid interface name, none given twice.
fn check_taps(taps: &[String]) -> Result<Vec<String>, String> {
    if taps.len() > MAX_TAPS {
        return Err(format!("at most {MAX_TAPS} --tap flags"));
    }
    for (i, tap) in taps.iter().enumerate() {
        if !valid_tap_name(tap) {
            return Err(format!("--tap {tap:?}: use 1 to 15 letters, digits, _ and -, not lo"));
        }
        if taps[..i].contains(tap) {
            return Err(format!("--tap {tap:?} given twice"));
        }
    }
    Ok(taps.to_vec())
}

/// `LAUNCHAPI <id> [ceiling] [stage=SRC:DEST]... [tap=NAME]...`.
fn launch_request(opts: &Options) -> String {
    let mut words = vec!["LAUNCHAPI".to_string(), opts.id.clone()];
    words.extend(opts.memory_max.map(|bytes| bytes.to_string()));
    words.extend(opts.stages.iter().map(StageSpec::launch_token));
    words.extend(opts.taps.iter().map(|tap| format!("tap={tap}")));
    words.join(" ")
}

/// Asks the agent for `DATA status` and refuses unless the data disk is `formatted` or `mounted` (ADR 0007 section
/// 5): jails and the snapshot store live on it.
async fn check_data_disk(target: &Target, config: &ServerConfig) -> Result<(), String> {
    let mut agent = proxy::open_upstream(target, config).await?;
    let greeting = proxy::read_line_bounded(&mut agent, MAX_LINE, Some(HANDSHAKE_TIMEOUT))
        .await
        .map_err(|err| format!("agent greeting: {err}"))?;
    if !greeting.starts_with(&greeting_prefix()) {
        return Err(format!("unexpected agent greeting {greeting:?}"));
    }
    proxy::write_line(&mut agent, "DATA status", Some(HANDSHAKE_TIMEOUT)).await?;
    let head = proxy::read_line_bounded(&mut agent, MAX_LINE, Some(HANDSHAKE_TIMEOUT))
        .await
        .map_err(|err| format!("DATA status: {err}"))?;
    let count = match head.split(' ').collect::<Vec<_>>().as_slice() {
        ["DATA", _, n] => n.parse::<usize>().ok().filter(|n| *n <= MAX_DATA_LINES),
        _ => None,
    }
    .ok_or_else(|| format!("DATA status: unexpected reply {head:?}"))?;
    let mut state = String::new();
    for _ in 0..count {
        let line = proxy::read_line_bounded(&mut agent, MAX_LINE, Some(HANDSHAKE_TIMEOUT))
            .await
            .map_err(|err| format!("DATA status: {err}"))?;
        if line.starts_with("DATA ") && state.is_empty() {
            state = line;
        }
    }
    if data_disk_in_use(&state) {
        Ok(())
    } else {
        Err(format!(
            "the System VM's data disk is not in use ({}); vesperd microVMs keep their jails and the snapshot store on it",
            if state.is_empty() { "no DATA line".to_string() } else { state }
        ))
    }
}

/// One `LAUNCHAPI` exchange; `Ok(Err(line))` is an `ERR busy` reply worth retrying.
async fn launch_once(
    opts: &Options,
    target: &Target,
    config: &ServerConfig,
) -> Result<Result<(Box<dyn proxy::Upstream>, Ready), String>, String> {
    let mut lease = proxy::open_upstream(target, config).await?;
    let greeting = proxy::read_line_bounded(&mut lease, MAX_LINE, Some(HANDSHAKE_TIMEOUT))
        .await
        .map_err(|err| format!("agent greeting: {err}"))?;
    if !greeting.starts_with(&greeting_prefix()) {
        return Err(format!("unexpected agent greeting {greeting:?}"));
    }
    let request = launch_request(opts);
    proxy::write_line(&mut lease, &request, Some(HANDSHAKE_TIMEOUT)).await?;
    let wait = if opts.stages.is_empty() { READY_TIMEOUT } else { READY_TIMEOUT + STAGE_ALLOWANCE };
    let reply = proxy::read_line_bounded(&mut lease, MAX_LINE, Some(wait))
        .await
        .map_err(|err| format!("LAUNCHAPI {}: {err}", opts.id))?;
    if reply == format!("ERR busy {}", opts.id) {
        return Ok(Err(reply));
    }
    let ready = parse_ready(&reply, &opts.id)?;
    Ok(Ok((lease, ready)))
}

/// `LAUNCHAPI`, retrying `ERR busy` for `REUSE_WAIT` while an earlier microVM with this id is torn down or reaped.
async fn launch(opts: &Options, target: &Target, config: &ServerConfig) -> Result<(Box<dyn proxy::Upstream>, Ready), String> {
    let started = Instant::now();
    loop {
        match launch_once(opts, target, config).await? {
            Ok(leased) => return Ok(leased),
            Err(busy) if started.elapsed() < REUSE_WAIT => {
                let _ = busy;
                tokio::time::sleep(REUSE_RETRY).await;
            }
            Err(busy) => {
                return Err(format!("LAUNCHAPI {}: {busy} after {} s; another microVM holds the id", opts.id, REUSE_WAIT.as_secs()));
            }
        }
    }
}

/// Resolves when the agent closes the lease (or it fails); bytes on it are discarded.
async fn lease_closed<R: AsyncRead + Unpin>(lease: &mut R) {
    let mut buf = [0u8; 256];
    loop {
        match lease.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(_) => continue,
        }
    }
}

/// Closes the lease (end of file stops the microVM) and waits for the agent to close its side after the teardown.
async fn close_lease<R, W>(read: &mut R, write: &mut W) -> Result<Duration, String>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let started = Instant::now();
    let _ = write.shutdown().await;
    tokio::time::timeout(STOP_WAIT, lease_closed(read))
        .await
        .map_err(|_| format!("the agent did not confirm the teardown within {} s", STOP_WAIT.as_secs()))?;
    Ok(started.elapsed())
}

/// A status line on stdout; a closed stdout must not stop the relay or the teardown (`println!` panics on it).
fn say(line: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}").and_then(|_| out.flush());
}

enum Ending {
    Signal(&'static str),
    LeaseClosed,
    Serve(Result<(), String>),
}

pub async fn run(opts: Options, socket: &Path) -> Result<(), String> {
    let pinned = vesper::spec::load_info_version(&opts.swagger)?;
    let config = relay_config(&opts, socket)?;
    let target = proxy::parse_target(&config.upstream_raw)?;
    // Registered before the launch, so a signal during it still ends in a clean stop.
    let mut term = signal(SignalKind::terminate()).map_err(|err| format!("register SIGTERM: {err}"))?;
    let mut int = signal(SignalKind::interrupt()).map_err(|err| format!("register SIGINT: {err}"))?;
    // Held for the life of the process; no microVM is launched for a socket another vesperd serves.
    let _lock = proxy::claim_socket(socket)?;
    check_data_disk(&target, &config).await?;

    let (lease, ready) = launch(&opts, &target, &config).await?;
    let (mut lease_read, mut lease_write) = tokio::io::split(lease);
    if ready.firecracker_version != pinned {
        let _ = close_lease(&mut lease_read, &mut lease_write).await;
        return Err(format!(
            "the System VM runs Firecracker {}, the pinned swagger is {pinned}; refusing to serve",
            ready.firecracker_version
        ));
    }
    let listener = match super::bind_owner_only(socket) {
        Ok(listener) => listener,
        Err(err) => {
            let _ = close_lease(&mut lease_read, &mut lease_write).await;
            return Err(err);
        }
    };
    say(&format!(
        "vesperd listening on {} (backend systemvm, microVM {}, Firecracker {} pid {})",
        socket.display(),
        opts.id,
        ready.firecracker_version,
        ready.pid
    ));

    let ending = tokio::select! {
        result = proxy::serve(listener, config, std::future::pending()) => Ending::Serve(result),
        _ = lease_closed(&mut lease_read) => Ending::LeaseClosed,
        _ = term.recv() => Ending::Signal("SIGTERM"),
        _ = int.recv() => Ending::Signal("SIGINT"),
    };
    // The listener is closed; nothing new reaches the microVM.
    let _ = std::fs::remove_file(socket);
    match ending {
        Ending::Signal(name) => {
            let took = close_lease(&mut lease_read, &mut lease_write)
                .await
                .map_err(|err| format!("{name}: closed the lease of microVM {}, but {err}", opts.id))?;
            say(&format!("vesperd: {name}: microVM {} stopped in {} ms", opts.id, took.as_millis()));
            Ok(())
        }
        Ending::LeaseClosed => Err(format!(
            "the lease of microVM {} closed: Firecracker exited or the System VM went away",
            opts.id
        )),
        Ending::Serve(result) => {
            let _ = close_lease(&mut lease_read, &mut lease_write).await;
            Err(result.err().unwrap_or_else(|| "the relay stopped".to_string()))
        }
    }
}
