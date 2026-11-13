use clap::{Parser, Subcommand};

#[path = "systemvmctl/replay.rs"]
mod replay;

const DEFAULT_IMAGE_LOCK: &str = "docs/specs/systemvm-image.lock.yaml";
const DEFAULT_IMAGE_DIR: &str = "build/systemvm";
const DEFAULT_PACKAGES_LOCK: &str = "docs/specs/systemvm-packages.lock.yaml";
const DEFAULT_MANIFEST: &str = "build/systemvm/image-manifest.json";

#[derive(Parser, Debug)]
#[command(name = "systemvmctl")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Digest kernel plus rootfs and/or initrd (and optional boot media) into an image manifest.
    Build {
        #[arg(long)]
        kernel: String,
        #[arg(long)]
        rootfs: Option<String>,
        #[arg(long)]
        initrd: Option<String>,
        #[arg(long)]
        boot_media: Option<String>,
        #[arg(long, default_value = DEFAULT_MANIFEST)]
        out: String,
    },
    /// Write the NAT-default launch configuration, from explicit paths or from the image lock.
    LaunchConfig {
        /// Derive kernel, initrd, boot media, cmdline and ready pattern from the lock and --dir.
        #[arg(long, conflicts_with_all = ["kernel", "rootfs", "initrd", "boot_media", "cmdline", "ready_pattern", "nested"])]
        from_lock: bool,
        #[arg(long, default_value = DEFAULT_IMAGE_LOCK)]
        lock: String,
        #[arg(long, default_value = DEFAULT_IMAGE_DIR)]
        dir: String,
        #[arg(long, required_unless_present = "from_lock")]
        kernel: Option<String>,
        #[arg(long)]
        rootfs: Option<String>,
        #[arg(long)]
        initrd: Option<String>,
        #[arg(long)]
        boot_media: Option<String>,
        /// Full kernel command line (default: console=hvc0, plus root= when a rootfs is given).
        #[arg(long)]
        cmdline: Option<String>,
        /// Console text that marks a healthy boot (default "login:").
        #[arg(long)]
        ready_pattern: Option<String>,
        /// Host directory to share into the System VM over virtio-fs as `PATH`, `PATH:ro` or `PATH:nowatch` (repeatable);
        /// the guest mounts it at the same absolute path (GH #29).
        #[arg(long = "share")]
        shares: Vec<String>,
        /// Nested virtualization policy: auto, require or off.
        #[arg(long)]
        nested: Option<String>,
        /// Persistent data disk image on macOS (absolute path), attached writable (GH #164).
        #[arg(long)]
        data_disk: Option<String>,
        #[arg(long, default_value = "run/systemvm/console.log")]
        console_log: String,
        #[arg(long, default_value = "build/systemvm/launch-config.json")]
        out: String,
    },
    /// Print the pinned image lock as shell `KEY='VALUE'` lines.
    LockEnv {
        #[arg(long, default_value = DEFAULT_IMAGE_LOCK)]
        lock: String,
    },
    /// Print `file<TAB>url<TAB>sha256` for every pinned package.
    PackageList {
        #[arg(long, default_value = DEFAULT_PACKAGES_LOCK)]
        lock: String,
    },
    /// Check downloaded packages (and extracted archive members) against the packages lock.
    VerifyPackages {
        #[arg(long, default_value = DEFAULT_PACKAGES_LOCK)]
        lock: String,
        #[arg(long, default_value = "build/systemvm/cache/apks")]
        dir: String,
        #[arg(long, default_value = "build/systemvm/cache/archives")]
        archives_dir: String,
    },
    /// Print one tab-separated line per pinned archive member.
    ArchiveList {
        #[arg(long, default_value = DEFAULT_PACKAGES_LOCK)]
        lock: String,
    },
    /// Check fetched artifacts against the lock and write the manifest.
    VerifyImage {
        #[arg(long, default_value = DEFAULT_IMAGE_LOCK)]
        lock: String,
        #[arg(long, default_value = DEFAULT_IMAGE_DIR)]
        dir: String,
        #[arg(long, default_value = DEFAULT_MANIFEST)]
        out: String,
        /// Fail when the lock has an overlay but the derived initramfs has not been built.
        #[arg(long)]
        require_overlay: bool,
    },
    /// Probe the host through the launcher and print the backend selection (GH #17).
    BackendSelect {
        #[arg(long, default_value = "build/vmlauncher/vesper-vmlauncher")]
        launcher: String,
        /// auto, require or off (default: the image lock's boot.nested_virtualization).
        #[arg(long)]
        policy: Option<String>,
        #[arg(long, default_value = DEFAULT_IMAGE_LOCK)]
        lock: String,
        /// Print the raw host capabilities instead of the selection.
        #[arg(long)]
        capabilities: bool,
    },
    /// Talk to the guest agent over the vsock bridge (GH #13).
    Control {
        /// Launch config that names the bridge socket and agent port.
        #[arg(long, default_value = "build/systemvm/launch-config.json")]
        config: String,
        #[arg(long)]
        bridge: Option<String>,
        #[arg(long)]
        port: Option<u32>,
        #[command(subcommand)]
        action: ControlAction,
    },
    BootSmoke {
        #[arg(long, default_value_t = 5)]
        iterations: u32,
        #[arg(long, default_value_t = 20)]
        timeout_seconds: u64,
        #[arg(long, default_value = "sh")]
        command: String,
        #[arg(long, value_delimiter = ',', default_values_t = vec!["-c".to_string(), "echo boot-ok".to_string()])]
        args: Vec<String>,
        #[arg(long, default_value = "run/systemvm/boot.log")]
        log: String,
        #[arg(long, default_value = "run/systemvm/boot-report.json")]
        report: String,
        /// Fail an iteration whose boot time exceeds this many milliseconds.
        #[arg(long)]
        max_boot_ms: Option<u64>,
    },
}

#[derive(Subcommand, Debug)]
enum ControlAction {
    /// Connect, print the greeting.
    Handshake,
    Ping,
    Version,
    Health,
    /// Send N bytes through ECHO and verify them.
    Echo {
        #[arg(long, default_value_t = 2 * 1024 * 1024)]
        bytes: usize,
    },
    /// Ping every INTERVAL seconds for SECONDS.
    Keepalive {
        #[arg(long, default_value_t = 5)]
        interval: u64,
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
    /// Ask the guest to power off.
    Shutdown,
    /// SIGKILL a supervised guest daemon (containerd or docker).
    Kill { service: String },
    /// Tail a guest log: messages, containerd or docker.
    Logs {
        log: String,
        #[arg(long, default_value_t = 50)]
        lines: u32,
    },
    /// Run the GH #14 checklist (healthy, crash, supervised recovery, logs) and write a JSON report.
    ServicesSuite {
        #[arg(long, default_value = "run/systemvm/services-report.json")]
        report: String,
        #[arg(long, value_delimiter = ',', default_values_t = vec!["containerd".to_string(), "docker".to_string()])]
        services: Vec<String>,
        #[arg(long, default_value_t = 60)]
        recovery_seconds: u64,
    },
    /// Run `vesper-microvm <args>` in the guest (prepare|launch ID [--exit]|status|isolation|stop|crash|cleanup|log).
    Microvm {
        args: Vec<String>,
    },
    /// Run `vesper-rootfs <args>` in the guest (import-trivial|build IMAGE [--no-cache]|provenance PATH).
    Rootfs {
        args: Vec<String>,
    },
    /// Run the GH #19 checklist (reproducible build, provenance, invalid image, boot in a microVM) and write a JSON report.
    RootfsSuite {
        #[arg(long, default_value = "run/systemvm/rootfs-report.json")]
        report: String,
        #[arg(long, default_value = "vesper-rootfs-test")]
        id: String,
    },
    /// Run `vesper-workload <args>` in the guest (run NAME IMAGE -- CMD...|ps|logs|wait|stop|rm|isolation).
    Workload {
        args: Vec<String>,
    },
    /// Run the GH #20 checklist (concurrent workloads, isolation, stop teardown, exit mapping) and write a JSON report.
    WorkloadSuite {
        #[arg(long, default_value = "run/systemvm/workload-report.json")]
        report: String,
    },
    /// Run `vesper-fcparity <args>` in the guest (api ID METHOD PATH [BODY] [TIMEOUT]|conformance ID|perf N).
    Fcparity {
        args: Vec<String>,
    },
    /// Run the GH #21 baseline (Firecracker API conformance plus two performance runs) and write a JSON report.
    ParitySuite {
        #[arg(long, default_value = "run/systemvm/parity-suite-report.json")]
        report: String,
        #[arg(long, default_value = "vesper-parity")]
        id: String,
        #[arg(long, default_value_t = 20)]
        perf_cycles: u32,
    },
    /// Run `vesper-arch <args>` in the guest (status|mode ARCH): native vs emulated execution modes.
    Arch {
        args: Vec<String>,
    },
    /// Run `vesper-share <args>` in the guest (mount TAG PATH [ro]|list|umount PATH|probe TAG PATH).
    Share {
        args: Vec<String>,
    },
    /// Run `vesper-data <args>` in the guest (mount|status|stop of the persistent data disk, GH #164).
    Data {
        args: Vec<String>,
    },
    /// Run `vesper-ctr <args>` in the guest (import|run NAME CMD...|start|state|kill|delete|try CTR-ARGS...|leftovers).
    Ctr {
        args: Vec<String>,
    },
    /// Run the GH #23 checklist (runc exec, streams, terminal resize and signals).
    ExecSuite {
        #[arg(long, default_value = "run/systemvm/exec-report.json")]
        report: String,
    },
    /// Run the GH #22 checklist (ctr task lifecycle through the Vesper shim v2) and write a JSON report.
    ShimSuite {
        #[arg(long, default_value = "run/systemvm/shim-report.json")]
        report: String,
    },
    /// Run the GH #24 checklist (block-first rootfs cache, layered image, metadata, dedup) and write a JSON report.
    BundleSuite {
        #[arg(long, default_value = "run/systemvm/bundle-report.json")]
        report: String,
    },
    /// Run the GH #25 checklist (containerd restart, shim crash cleanup, in-flight failure, durable state) and write a JSON report.
    RestartSuite {
        #[arg(long, default_value = "run/systemvm/restart-report.json")]
        report: String,
    },
    /// Run `vesper-vesperd <args>` in the guest (status|isolation|list|log|reap|kill-holder|kill-fc ID|test-…; ADR 0007).
    Vesperd {
        args: Vec<String>,
    },
    /// Run the ADR 0007 slice 3 checklist (LAUNCHAPI, FCAPI, boot through the API, teardowns, reaper) and write a JSON report.
    LaunchapiSuite {
        #[arg(long, default_value = "run/systemvm/launchapi-report.json")]
        report: String,
        #[arg(long, default_value = "vesper-launchapi")]
        id: String,
        /// The pinned swagger whose info.version READY must report.
        #[arg(long, default_value = "third_party/firecracker/firecracker.yaml")]
        swagger: String,
    },
    /// Run the ADR 0007 slice 4 checklist through `vesperd --backend systemvm`'s own socket and write a JSON report.
    VesperdSuite {
        #[arg(long, default_value = "run/systemvm/vesperd-report.json")]
        report: String,
        #[arg(long, default_value = "vesper-vesperd")]
        id: String,
        /// The pinned swagger whose info.version vesperd must report.
        #[arg(long, default_value = "third_party/firecracker/firecracker.yaml")]
        swagger: String,
        /// The vesperd binary to start (`make systemvm-vesperd-suite` builds it).
        #[arg(long, default_value = "target/debug/vesperd")]
        vesperd: String,
        /// Owner-only directory for the suite's vesperd sockets; short, because macOS limits socket paths to 104 bytes.
        #[arg(long, default_value = "run/vesperd-suite")]
        workdir: String,
    },
    /// Run the TD-055 and TD-056 checklist: a console flood stays within the bounded log, the reaper prunes old logs,
    /// and one memory budget admits `vesperd` and shim microVMs together.
    BoundsSuite {
        #[arg(long, default_value = "run/systemvm/bounds-report.json")]
        report: String,
        #[arg(long, default_value = "vesper-bounds")]
        id: String,
        /// The vesperd binary to start (`make systemvm-bounds-suite` builds it).
        #[arg(long, default_value = "target/debug/vesperd")]
        vesperd: String,
        #[arg(long, default_value = "run/bounds-suite")]
        workdir: String,
    },
    /// Run the ADR 0008 egress checklist: `vesperd --network nat` guests probe from inside (public only, DNS, the
    /// System VM, the Mac, containers, other leases, spoofing), the root layer alone after a dockerd restart, teardown.
    EgressSuite {
        #[arg(long, default_value = "run/systemvm/egress-report.json")]
        report: String,
        #[arg(long, default_value = "vesper-egress")]
        id: String,
        /// The vesperd binary to start (`make systemvm-egress-suite` builds it).
        #[arg(long, default_value = "target/debug/vesperd")]
        vesperd: String,
        #[arg(long, default_value = "run/egress-suite")]
        workdir: String,
    },
    /// Run the ADR 0007 slice 5 checklist (staging rules, the store, the snapshot cycle, the ceiling measurement)
    /// through `vesperd`; the launch config must share `--share-dir`.
    SnapshotSuite {
        #[arg(long, default_value = "run/systemvm/snapshot-report.json")]
        report: String,
        #[arg(long, default_value = "vesper-snap")]
        id: String,
        /// The vesperd binary to start (`make systemvm-snapshot-suite` builds it).
        #[arg(long, default_value = "target/debug/vesperd")]
        vesperd: String,
        /// Owner-only directory for the suite's vesperd sockets.
        #[arg(long, default_value = "run/vesperd-suite")]
        workdir: String,
        /// A directory inside a share of the launch config (`make systemvm-up-snapshot` shares run/vesperd-share).
        #[arg(long, default_value = "run/vesperd-share")]
        share_dir: String,
    },
    /// Replay the behaviour matrix through `vesperd` on Backend A (ADR 0007 slice 6): one microVM per case, judged
    /// against the matrix; the launch config must share `--share-dir`.
    ReplaySuite {
        #[arg(long, default_value = "run/systemvm/replay-report.json")]
        report: String,
        /// Also write the committed summary (`make systemvm-replay-publish`).
        #[arg(long)]
        publish: Option<String>,
        #[arg(long, default_value = vesper::api_replay::DEFAULT_REPLAY_SCRIPTS_PATH)]
        scripts: String,
        #[arg(long, default_value = "docs/generated/firecracker-api-behavior-matrix.json")]
        matrix: String,
        /// Only cases whose `METHOD PATH#id` key contains this text.
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, default_value = "vesper-rp")]
        id: String,
        #[arg(long, default_value = "target/debug/vesperd")]
        vesperd: String,
        #[arg(long, default_value = "run/vesperd-suite")]
        workdir: String,
        #[arg(long, default_value = "run/vesperd-share")]
        share_dir: String,
    },
    /// Run the GH #18 checklist (launch, jailer isolation, stop, crash, poweroff, cleanup) and write a JSON report.
    MicrovmSuite {
        #[arg(long, default_value = "run/systemvm/microvm-report.json")]
        report: String,
        #[arg(long, default_value = "vesper-test")]
        id: String,
    },
    /// Run the GH #13 checklist and write a JSON report.
    Suite {
        #[arg(long, default_value = "run/systemvm/control-report.json")]
        report: String,
        #[arg(long, default_value_t = 2)]
        keepalive_interval: u64,
        #[arg(long, default_value_t = 20)]
        keepalive_seconds: u64,
        #[arg(long, default_value_t = 2 * 1024 * 1024)]
        payload_bytes: usize,
    },
}

fn control_endpoint(
    config: &str,
    bridge: Option<String>,
    port: Option<u32>,
) -> Result<vesper::control::ControlEndpoint, String> {
    let (cfg_bridge, cfg_port) = match std::fs::read_to_string(config) {
        Ok(text) => {
            let cfg: vesper::systemvm::LaunchConfig = serde_json::from_str(&text)
                .map_err(|err| format!("parse launch config {config}: {err}"))?;
            (cfg.vsock_bridge_path, cfg.agent_port)
        }
        Err(_) if bridge.is_some() => (None, None),
        Err(err) => return Err(format!("read launch config {config}: {err} (pass --bridge/--port or run make systemvm-launch-config)")),
    };
    let bridge = bridge
        .or(cfg_bridge)
        .ok_or("no bridge socket: the launch config has no vsock_bridge_path and --bridge was not given")?;
    let port = port
        .or(cfg_port)
        .unwrap_or(vesper::control::DEFAULT_AGENT_PORT);
    Ok(vesper::control::ControlEndpoint::new(bridge, port))
}

fn print_steps(result: &vesper::control::SuiteReport, width: usize) {
    for step in &result.steps {
        println!(
            "{} {}: {} ({} ms)",
            if step.ok { "ok  " } else { "FAIL" },
            step.name,
            step.detail.chars().take(width).collect::<String>(),
            step.elapsed_ms
        );
    }
}

fn run_control(endpoint: vesper::control::ControlEndpoint, action: ControlAction) -> Result<(), String> {
    use vesper::control;
    match action {
        ControlAction::Handshake => {
            let session = control::connect(&endpoint)?;
            println!("{}", serde_json::to_string(&session.handshake).unwrap());
        }
        ControlAction::Ping => {
            let mut session = control::connect(&endpoint)?;
            println!("PONG in {} ms", session.ping()?.as_millis());
        }
        ControlAction::Version => {
            let mut session = control::connect(&endpoint)?;
            println!("VERSION {}", session.version()?);
        }
        ControlAction::Health => {
            let mut session = control::connect(&endpoint)?;
            println!("{}", session.health()?);
        }
        ControlAction::Echo { bytes } => {
            let mut session = control::connect(&endpoint)?;
            let payload: Vec<u8> = (0..bytes).map(|i| (i % 251) as u8).collect();
            let elapsed = session.echo(&payload)?;
            println!("echoed {bytes} bytes intact in {} ms", elapsed.as_millis());
        }
        ControlAction::Keepalive { interval, seconds } => {
            let mut session = control::connect(&endpoint)?;
            let pings = session.keepalive(
                std::time::Duration::from_secs(interval),
                std::time::Duration::from_secs(seconds),
            )?;
            println!("keepalive ok: {pings} pings over {seconds} s");
        }
        ControlAction::Shutdown => {
            let mut session = control::connect(&endpoint)?;
            session.shutdown()?;
            println!("guest acknowledged shutdown");
        }
        ControlAction::Microvm { args } => {
            let mut session = control::connect(&endpoint)?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (rc, lines) = session.microvm(&refs)?;
            for line in &lines {
                println!("{line}");
            }
            if rc != 0 {
                return Err(format!("vesper-microvm exited with {rc}"));
            }
        }
        ControlAction::Rootfs { args } => {
            let mut session = control::connect(&endpoint)?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (rc, lines) = session.rootfs(&refs)?;
            for line in &lines {
                println!("{line}");
            }
            if rc != 0 {
                return Err(format!("vesper-rootfs exited with {rc}"));
            }
        }
        ControlAction::RootfsSuite { report, id } => {
            let result = control::run_rootfs_suite(&endpoint, &id);
            control::write_report(&report, &result)?;
            for step in &result.steps {
                println!(
                    "{} {}: {} ({} ms)",
                    if step.ok { "ok  " } else { "FAIL" },
                    step.name,
                    step.detail.chars().take(220).collect::<String>(),
                    step.elapsed_ms
                );
            }
            if !result.passed {
                return Err(format!("rootfs suite failed (report: {report})"));
            }
            println!("rootfs suite passed (report: {report})");
        }
        ControlAction::Workload { args } => {
            let mut session = control::connect(&endpoint)?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (rc, lines) = session.workload(&refs)?;
            for line in &lines {
                println!("{line}");
            }
            if rc != 0 {
                return Err(format!("vesper-workload exited with {rc}"));
            }
        }
        ControlAction::WorkloadSuite { report } => {
            let result = control::run_workload_suite(&endpoint);
            control::write_report(&report, &result)?;
            for step in &result.steps {
                println!(
                    "{} {}: {} ({} ms)",
                    if step.ok { "ok  " } else { "FAIL" },
                    step.name,
                    step.detail.chars().take(220).collect::<String>(),
                    step.elapsed_ms
                );
            }
            if !result.passed {
                return Err(format!("workload suite failed (report: {report})"));
            }
            println!("workload suite passed (report: {report})");
        }
        ControlAction::Fcparity { args } => {
            let mut session = control::connect(&endpoint)?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (rc, lines) = session.fcparity(&refs)?;
            for line in &lines {
                println!("{line}");
            }
            if rc != 0 {
                return Err(format!("vesper-fcparity exited with {rc}"));
            }
        }
        ControlAction::ParitySuite { report, id, perf_cycles } => {
            let result = control::run_parity_suite(&endpoint, &id, perf_cycles);
            control::write_report(&report, &result)?;
            for step in &result.steps {
                println!(
                    "{} {}: {} ({} ms)",
                    if step.ok { "ok  " } else { "FAIL" },
                    step.name,
                    step.detail.chars().take(220).collect::<String>(),
                    step.elapsed_ms
                );
            }
            if !result.passed {
                return Err(format!("parity suite failed (report: {report})"));
            }
            println!("parity suite passed (report: {report})");
        }
        ControlAction::Arch { args } => {
            let mut session = control::connect(&endpoint)?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (rc, lines) = session.arch(&refs)?;
            for line in &lines {
                println!("{line}");
            }
            if rc != 0 {
                return Err(format!("vesper-arch exited with {rc}"));
            }
        }
        ControlAction::Vesperd { args } => {
            let mut session = control::connect(&endpoint)?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (rc, lines) = session.vesperd(&refs)?;
            for line in &lines {
                println!("{line}");
            }
            if rc != 0 {
                return Err(format!("vesper-vesperd exited with {rc}"));
            }
        }
        ControlAction::LaunchapiSuite { report, id, swagger } => {
            let version = vesper::spec::load_info_version(&swagger)?;
            let result = control::run_launchapi_suite(&endpoint, &id, &version);
            control::write_report(&report, &result)?;
            print_steps(&result, 220);
            if !result.passed {
                return Err(format!("launchapi suite failed (report: {report})"));
            }
            println!("launchapi suite passed (report: {report})");
        }
        ControlAction::VesperdSuite { report, id, swagger, vesperd, workdir } => {
            let version = vesper::spec::load_info_version(&swagger)?;
            let run = control::VesperdRun {
                binary: vesperd.into(),
                workdir: workdir.into(),
                extra_args: vec![
                    "--bridge".to_string(),
                    endpoint.bridge_path.to_string_lossy().into_owned(),
                    "--agent-port".to_string(),
                    endpoint.port.to_string(),
                    "--swagger".to_string(),
                    swagger.clone(),
                ],
            };
            let result = control::run_vesperd_suite(&endpoint, &run, &id, &version);
            control::write_report(&report, &result)?;
            print_steps(&result, 260);
            if !result.passed {
                return Err(format!("vesperd suite failed (report: {report})"));
            }
            println!("vesperd suite passed (report: {report})");
        }
        ControlAction::BoundsSuite { report, id, vesperd, workdir } => {
            let run = control::VesperdRun {
                binary: vesperd.into(),
                workdir: workdir.clone().into(),
                extra_args: vec![
                    "--bridge".to_string(),
                    endpoint.bridge_path.to_string_lossy().into_owned(),
                    "--agent-port".to_string(),
                    endpoint.port.to_string(),
                ],
            };
            control::prepare_workdir(std::path::Path::new(&workdir))?;
            let result = control::run_bounds_suite(&endpoint, &run, &id);
            control::write_report(&report, &result)?;
            print_steps(&result, 300);
            if !result.passed {
                return Err(format!("bounds suite failed (report: {report})"));
            }
            println!("bounds suite passed (report: {report})");
        }
        ControlAction::EgressSuite { report, id, vesperd, workdir } => {
            let run = control::VesperdRun {
                binary: vesperd.into(),
                workdir: workdir.clone().into(),
                extra_args: vec![
                    "--bridge".to_string(),
                    endpoint.bridge_path.to_string_lossy().into_owned(),
                    "--agent-port".to_string(),
                    endpoint.port.to_string(),
                ],
            };
            control::prepare_workdir(std::path::Path::new(&workdir))?;
            let result = control::run_egress_suite(&endpoint, &run, &id);
            control::write_report(&report, &result)?;
            print_steps(&result, 300);
            if !result.passed {
                return Err(format!("egress suite failed (report: {report})"));
            }
            println!("egress suite passed (report: {report})");
        }
        ControlAction::ReplaySuite { report, publish, scripts, matrix, filter, id, vesperd, workdir, share_dir } => {
            let run = control::ReplayRun {
                vesperd: control::VesperdRun {
                    binary: vesperd.into(),
                    workdir: workdir.clone().into(),
                    extra_args: vec![
                        "--bridge".to_string(),
                        endpoint.bridge_path.to_string_lossy().into_owned(),
                        "--agent-port".to_string(),
                        endpoint.port.to_string(),
                    ],
                },
                share_dir: {
                    std::fs::create_dir_all(&share_dir).map_err(|err| format!("create {share_dir}: {err}"))?;
                    std::fs::canonicalize(&share_dir).map_err(|err| format!("{share_dir}: {err}"))?
                },
            };
            control::prepare_workdir(std::path::Path::new(&workdir))?;
            replay::run(&endpoint, &run, &scripts, &matrix, filter.as_deref(), &id, &report, publish.as_deref())?;
        }
        ControlAction::SnapshotSuite { report, id, vesperd, workdir, share_dir } => {
            control::prepare_workdir(std::path::Path::new(&workdir))?;
            std::fs::create_dir_all(&share_dir).map_err(|err| format!("create {share_dir}: {err}"))?;
            let share_dir = std::fs::canonicalize(&share_dir).map_err(|err| format!("{share_dir}: {err}"))?;
            let run = control::SnapshotRun {
                vesperd: control::VesperdRun {
                    binary: vesperd.into(),
                    workdir: workdir.into(),
                    extra_args: vec![
                        "--bridge".to_string(),
                        endpoint.bridge_path.to_string_lossy().into_owned(),
                        "--agent-port".to_string(),
                        endpoint.port.to_string(),
                    ],
                },
                share_dir,
            };
            let result = control::run_snapshot_suite(&endpoint, &run, &id);
            control::write_report(&report, &result)?;
            print_steps(&result, 320);
            if !result.passed {
                return Err(format!("snapshot suite failed (report: {report})"));
            }
            println!("snapshot suite passed (report: {report})");
        }
        ControlAction::Data { args } => {
            let mut session = control::connect(&endpoint)?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (rc, lines) = session.data(&refs)?;
            for line in &lines {
                println!("{line}");
            }
            if rc != 0 {
                return Err(format!("vesper-data exited with {rc}"));
            }
        }
        ControlAction::Share { args } => {
            let mut session = control::connect(&endpoint)?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (rc, lines) = session.share(&refs)?;
            for line in &lines {
                println!("{line}");
            }
            if rc != 0 {
                return Err(format!("vesper-share exited with {rc}"));
            }
        }
        ControlAction::Ctr { args } => {
            let mut session = control::connect(&endpoint)?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let (rc, lines) = session.ctr(&refs)?;
            for line in &lines {
                println!("{line}");
            }
            if rc != 0 {
                return Err(format!("vesper-ctr exited with {rc}"));
            }
        }
        ControlAction::ExecSuite { report } => {
            let result = control::run_exec_suite(&endpoint);
            control::write_report(&report, &result)?;
            for step in &result.steps {
                println!(
                    "{} {}: {} ({} ms)",
                    if step.ok { "ok  " } else { "FAIL" },
                    step.name,
                    step.detail.chars().take(220).collect::<String>(),
                    step.elapsed_ms
                );
            }
            if !result.passed {
                return Err(format!("exec suite failed (report: {report})"));
            }
            println!("exec suite passed (report: {report})");
        }
        ControlAction::ShimSuite { report } => {
            let result = control::run_shim_suite(&endpoint);
            control::write_report(&report, &result)?;
            print_steps(&result, 220);
            if !result.passed {
                return Err(format!("shim suite failed (report: {report})"));
            }
            println!("shim suite passed (report: {report})");
        }
        ControlAction::BundleSuite { report } => {
            let result = control::run_bundle_suite(&endpoint);
            control::write_report(&report, &result)?;
            print_steps(&result, 220);
            if !result.passed {
                return Err(format!("bundle suite failed (report: {report})"));
            }
            println!("bundle suite passed (report: {report})");
        }
        ControlAction::RestartSuite { report } => {
            let result = control::run_restart_suite(&endpoint);
            control::write_report(&report, &result)?;
            print_steps(&result, 220);
            if !result.passed {
                return Err(format!("restart suite failed (report: {report})"));
            }
            println!("restart suite passed (report: {report})");
        }
        ControlAction::MicrovmSuite { report, id } => {
            let result = control::run_microvm_suite(&endpoint, &id);
            control::write_report(&report, &result)?;
            for step in &result.steps {
                println!(
                    "{} {}: {} ({} ms)",
                    if step.ok { "ok  " } else { "FAIL" },
                    step.name,
                    step.detail.chars().take(200).collect::<String>(),
                    step.elapsed_ms
                );
            }
            if !result.passed {
                return Err(format!("microvm suite failed (report: {report})"));
            }
            println!("microvm suite passed (report: {report})");
        }
        ControlAction::Kill { service } => {
            let mut session = control::connect(&endpoint)?;
            println!("killed {service} pid {}", session.kill_service(&service)?);
        }
        ControlAction::Logs { log, lines } => {
            let mut session = control::connect(&endpoint)?;
            for line in session.logs(&log, lines)? {
                println!("{line}");
            }
        }
        ControlAction::ServicesSuite {
            report,
            services,
            recovery_seconds,
        } => {
            let names: Vec<&str> = services.iter().map(String::as_str).collect();
            let result = control::run_services_suite(
                &endpoint,
                &names,
                std::time::Duration::from_secs(recovery_seconds),
            );
            control::write_report(&report, &result)?;
            for step in &result.steps {
                println!(
                    "{} {}: {} ({} ms)",
                    if step.ok { "ok  " } else { "FAIL" },
                    step.name,
                    step.detail.chars().take(160).collect::<String>(),
                    step.elapsed_ms
                );
            }
            if !result.passed {
                return Err(format!("services suite failed (report: {report})"));
            }
            println!("services suite passed (report: {report})");
        }
        ControlAction::Suite {
            report,
            keepalive_interval,
            keepalive_seconds,
            payload_bytes,
        } => {
            let result = control::run_suite(
                &endpoint,
                std::time::Duration::from_secs(keepalive_interval),
                std::time::Duration::from_secs(keepalive_seconds),
                payload_bytes,
            );
            control::write_report(&report, &result)?;
            for step in &result.steps {
                println!(
                    "{} {}: {} ({} ms)",
                    if step.ok { "ok  " } else { "FAIL" },
                    step.name,
                    step.detail,
                    step.elapsed_ms
                );
            }
            if !result.passed {
                return Err(format!("control suite failed (report: {report})"));
            }
            println!("control suite passed (report: {report})");
        }
    }
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build {
            kernel,
            rootfs,
            initrd,
            boot_media,
            out,
        } => {
            let manifest = vesper::systemvm::build_manifest(&vesper::systemvm::ImageBuildConfig {
                kernel_path: kernel,
                rootfs_path: rootfs,
                initrd_path: initrd,
                boot_media_path: boot_media,
            })?;
            vesper::systemvm::write_manifest(&out, &manifest)?;
            println!("wrote image manifest: {out} (image_id={})", manifest.image_id);
            Ok(())
        }
        Commands::LaunchConfig {
            from_lock,
            lock,
            dir,
            kernel,
            rootfs,
            initrd,
            boot_media,
            cmdline,
            ready_pattern,
            nested,
            console_log,
            out,
            shares,
            data_disk,
        } => {
            let mut config = if from_lock {
                let lock = vesper::systemvm::load_image_lock(&lock)?;
                vesper::systemvm::launch_config_from_lock(&lock, &dir, &console_log)?
            } else {
                vesper::systemvm::default_launch_config(&vesper::systemvm::LaunchInputs {
                    kernel_path: kernel.as_deref().unwrap_or_default(),
                    rootfs_path: rootfs.as_deref(),
                    initrd_path: initrd.as_deref(),
                    boot_media_path: boot_media.as_deref(),
                    kernel_cmdline: cmdline.as_deref(),
                    ready_pattern: ready_pattern.as_deref(),
                    max_boot_ms: None,
                    probe: None,
                    vsock_bridge_path: None,
                    agent_port: None,
                    nested_virtualization: nested.as_deref(),
                    console_log_path: &console_log,
                })?
            };
            config.shared_directories = vesper::systemvm::shared_directories_from_specs(&shares)?;
            if let Some(path) = data_disk.as_deref() {
                vesper::systemvm::attach_data_disk(&mut config, path)?;
            }
            vesper::systemvm::write_launch_config(&out, &config)?;
            println!("wrote launch config: {out} ({} shared director{})", config.shared_directories.len(), if config.shared_directories.len() == 1 { "y" } else { "ies" });
            Ok(())
        }
        Commands::LockEnv { lock } => {
            let lock = vesper::systemvm::load_image_lock(&lock)?;
            print!("{}", vesper::systemvm::image_lock_env(&lock));
            Ok(())
        }
        Commands::PackageList { lock } => {
            let lock = vesper::systemvm::load_packages_lock(&lock)?;
            print!("{}", vesper::systemvm::packages_list(&lock));
            Ok(())
        }
        Commands::VerifyPackages { lock, dir, archives_dir } => {
            let lock = vesper::systemvm::load_packages_lock(&lock)?;
            let count = vesper::systemvm::verify_packages(&lock, &dir)?;
            let members = vesper::systemvm::verify_archives(&lock, &archives_dir)?;
            println!("verified {count} pinned packages in {dir} and {members} archive members in {archives_dir}");
            Ok(())
        }
        Commands::ArchiveList { lock } => {
            let lock = vesper::systemvm::load_packages_lock(&lock)?;
            print!("{}", vesper::systemvm::archive_list(&lock)?);
            Ok(())
        }
        Commands::VerifyImage {
            lock,
            dir,
            out,
            require_overlay,
        } => {
            let lock = vesper::systemvm::load_image_lock(&lock)?;
            let manifest = vesper::systemvm::verify_image(&lock, &dir, require_overlay)?;
            vesper::systemvm::write_manifest(&out, &manifest)?;
            println!(
                "verified {} {} {} image in {dir}: image_id={} (manifest: {out})",
                lock.distribution, lock.release, lock.arch, manifest.image_id
            );
            Ok(())
        }
        Commands::BackendSelect {
            launcher,
            policy,
            lock,
            capabilities,
        } => {
            let caps = vesper::platform::probe_host_capabilities(std::path::Path::new(&launcher))?;
            if capabilities {
                println!("{}", serde_json::to_string(&caps).unwrap());
                return Ok(());
            }
            let policy = match policy {
                Some(value) => vesper::platform::NestedPolicy::parse(&value)?,
                None => vesper::platform::NestedPolicy::parse(
                    &vesper::systemvm::load_image_lock(&lock)?.boot.nested_virtualization,
                )?,
            };
            let selection = vesper::platform::select_backend(&caps, policy)?;
            println!("{}", serde_json::to_string_pretty(&selection).unwrap());
            Ok(())
        }
        Commands::Control {
            config,
            bridge,
            port,
            action,
        } => run_control(control_endpoint(&config, bridge, port)?, action),
        Commands::BootSmoke {
            iterations,
            timeout_seconds,
            command,
            args,
            log,
            report,
            max_boot_ms,
        } => {
            let report_data =
                vesper::systemvm::run_boot_check(&vesper::systemvm::BootCheckConfig {
                    iterations,
                    timeout_seconds,
                    command,
                    args,
                    log_path: log,
                    report_path: report,
                    max_boot_ms,
                })?;
            println!(
                "boot smoke passed: successful={} failed={} max_boot_ms={} (threshold {})",
                report_data.successful,
                report_data.failed,
                report_data.max_boot_ms,
                report_data
                    .max_boot_allowed_ms
                    .map(|ms| ms.to_string())
                    .unwrap_or_else(|| "none".to_string())
            );
            Ok(())
        }
    }
}
