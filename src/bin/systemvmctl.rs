use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "systemvmctl")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Build {
        #[arg(long)]
        kernel: String,
        #[arg(long)]
        rootfs: String,
        #[arg(long, default_value = "build/systemvm/image-manifest.json")]
        out: String,
    },
    LaunchConfig {
        #[arg(long)]
        kernel: String,
        #[arg(long)]
        rootfs: String,
        #[arg(long, default_value = "run/systemvm/console.log")]
        console_log: String,
        #[arg(long, default_value = "build/systemvm/launch-config.json")]
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
            out,
        } => {
            let manifest = vesper::systemvm::build_manifest(&vesper::systemvm::ImageBuildConfig {
                kernel_path: kernel,
                rootfs_path: rootfs,
            })?;
            vesper::systemvm::write_manifest(&out, &manifest)?;
            println!("wrote image manifest: {out}");
            Ok(())
        }
        Commands::LaunchConfig {
            kernel,
            rootfs,
            console_log,
            out,
        } => {
            let config = vesper::systemvm::default_launch_config(&kernel, &rootfs, &console_log)?;
            vesper::systemvm::write_launch_config(&out, &config)?;
            println!("wrote launch config: {out}");
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
