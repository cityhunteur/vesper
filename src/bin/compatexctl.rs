use chrono::{NaiveDate, Utc};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "compatexctl")]
struct Args {
    #[arg(long, default_value = "docs/specs/compat-exceptions.yaml")]
    registry: String,
    #[arg(long)]
    today: Option<String>,
    #[arg(long, default_value_t = false)]
    require_zero_open: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    let today = match args.today {
        Some(value) => NaiveDate::parse_from_str(&value, "%Y-%m-%d")
            .map_err(|err| format!("invalid --today value: {err}"))?,
        None => Utc::now().date_naive(),
    };

    let registry = vesper::compat::load(&args.registry)?;
    let summary = vesper::compat::validate(&registry, today, args.require_zero_open)?;

    println!(
        "compat exceptions valid: open={} closed={} approved={}",
        summary.open, summary.closed, summary.approved
    );

    Ok(())
}
