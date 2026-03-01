use clap::Parser;
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(name = "contractgen")]
struct Args {
    #[arg(long, default_value = "generate")]
    mode: String,
    #[arg(long, default_value = "third_party/firecracker/firecracker.yaml")]
    swagger: String,
    #[arg(long, default_value = "testdata/contracts/firecracker-routes.golden.json")]
    out: String,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    let routes = vesper::spec::load_routes(&args.swagger)?;

    let mut content =
        serde_json::to_string_pretty(&routes).map_err(|err| format!("serialize routes: {err}"))?;
    content.push('\n');

    match args.mode.as_str() {
        "generate" => {
            if let Some(parent) = Path::new(&args.out).parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("create golden output directory {}: {err}", parent.display())
                })?;
            }
            fs::write(&args.out, content)
                .map_err(|err| format!("write golden file {}: {err}", &args.out))?;
            println!("generated {}", &args.out);
            Ok(())
        }
        "check" => {
            let current = fs::read_to_string(&args.out)
                .map_err(|err| format!("read golden file {}: {err}", &args.out))?;
            if current != content {
                return Err(
                    "golden contract is out of date; run: cargo run --bin contractgen -- --mode generate"
                        .to_string(),
                );
            }
            println!("golden contract is up to date: {}", &args.out);
            Ok(())
        }
        other => Err(format!("invalid mode {other:?} (expected generate|check)")),
    }
}
