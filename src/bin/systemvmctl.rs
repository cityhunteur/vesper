use clap::{Parser, Subcommand};

const DEFAULT_IMAGE_LOCK: &str = "docs/specs/systemvm-image.lock.yaml";
const DEFAULT_IMAGE_DIR: &str = "build/systemvm";
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
        #[arg(long, conflicts_with_all = ["kernel", "rootfs", "initrd", "boot_media", "cmdline", "ready_pattern"])]
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
    /// Check fetched artifacts against the lock and write the manifest.
    VerifyImage {
        #[arg(long, default_value = DEFAULT_IMAGE_LOCK)]
        lock: String,
        #[arg(long, default_value = DEFAULT_IMAGE_DIR)]
        dir: String,
        #[arg(long, default_value = DEFAULT_MANIFEST)]
        out: String,
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
    },
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
            console_log,
            out,
        } => {
            let config = if from_lock {
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
                    console_log_path: &console_log,
                })?
            };
            vesper::systemvm::write_launch_config(&out, &config)?;
            println!("wrote launch config: {out}");
            Ok(())
        }
        Commands::LockEnv { lock } => {
            let lock = vesper::systemvm::load_image_lock(&lock)?;
            print!("{}", vesper::systemvm::image_lock_env(&lock));
            Ok(())
        }
        Commands::VerifyImage { lock, dir, out } => {
            let lock = vesper::systemvm::load_image_lock(&lock)?;
            let manifest = vesper::systemvm::verify_image(&lock, &dir)?;
            vesper::systemvm::write_manifest(&out, &manifest)?;
            println!(
                "verified {} {} {} image in {dir}: image_id={} (manifest: {out})",
                lock.distribution, lock.release, lock.arch, manifest.image_id
            );
            Ok(())
        }
        Commands::BootSmoke {
            iterations,
            timeout_seconds,
            command,
            args,
            log,
            report,
        } => {
            let report_data =
                vesper::systemvm::run_boot_check(&vesper::systemvm::BootCheckConfig {
                    iterations,
                    timeout_seconds,
                    command,
                    args,
                    log_path: log,
                    report_path: report,
                })?;
            println!(
                "boot smoke passed: successful={} failed={}",
                report_data.successful, report_data.failed
            );
            Ok(())
        }
    }
}
