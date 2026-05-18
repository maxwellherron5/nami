//! The `nami` command-line entry point.
//!
//! Phase 0: argument parsing only. Each subcommand handler returns
//! `unimplemented!()`; the parsing surface is the contract we want to lock
//! down first. Implementation lands in subsequent sessions per the order
//! in `CLAUDE.md`'s "Phase 0 implementation goals".

use anyhow::Result;
use clap::{Parser, Subcommand};
use time::Duration;
use tracing_subscriber::EnvFilter;

use nami_core::Region;

mod preview;
mod run;
mod sink;

/// Conservative, uncertainty-aware, public-data carbon-aware scheduler.
#[derive(Debug, Parser)]
#[command(name = "nami", version, about, long_about = None)]
struct Cli {
    /// Increase log verbosity (`-v`, `-vv`).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Schedule and run a command in an estimated lower-carbon window
    /// before `--deadline`.
    Run(RunArgs),

    /// Compute a recommendation without executing anything.
    Preview(RunArgs),

    /// Print historical-pattern forecast points for a region and confidence
    /// metadata.
    Forecast(ForecastArgs),

    /// Print cache freshness, supported regions, provider availability,
    /// and configured data sources.
    Status(StatusArgs),
}

/// Args for `nami run` and `nami preview`.
#[derive(Debug, clap::Args)]
struct RunArgs {
    /// How long the job is expected to take, e.g. `2h`, `90m`, `45s`.
    #[arg(long, value_parser = parse_duration)]
    pub(crate) duration: Duration,

    /// Latest UTC instant the job is allowed to *finish*, RFC 3339 format.
    #[arg(long, value_parser = parse_datetime)]
    pub(crate) deadline: time::OffsetDateTime,

    /// Grid region (one of: CAISO, ERCOT, MISO, PJM, NYISO, ISONE, SPP).
    /// If omitted, region detection will be attempted.
    #[arg(long)]
    pub(crate) region: Option<Region>,

    /// Path to write the JSON run report. If omitted, the report goes to
    /// stdout at the end of the run.
    #[arg(long)]
    pub(crate) report: Option<std::path::PathBuf>,

    /// Silence the wrapped command's stdout and stderr (`nami run` only).
    /// `nami`'s own decision summary is still printed.
    #[arg(long, default_value_t = false)]
    pub(crate) quiet: bool,

    /// Redirect the wrapped command's stdout and stderr to this file
    /// (`nami run` only). Mutually exclusive with `--quiet`.
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) log: Option<std::path::PathBuf>,

    /// The command to wrap. Everything after `--` is forwarded verbatim.
    #[arg(last = true, required = true)]
    pub(crate) command: Vec<String>,
}

/// Args for `nami status`.
#[derive(Debug, clap::Args)]
struct StatusArgs {
    /// Optional path to a previously written run report; if provided, also
    /// summarize that report's decision and provenance.
    #[arg(long)]
    report: Option<std::path::PathBuf>,
}

/// Args for `nami forecast`.
#[derive(Debug, clap::Args)]
struct ForecastArgs {
    /// Grid region to query.
    #[arg(long)]
    region: Region,

    /// Forecast horizon, e.g. `24h`. Defaults to 24h.
    #[arg(long, value_parser = parse_duration, default_value = "24h")]
    horizon: Duration,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let (num_str, unit) = s
        .strip_suffix(|c: char| c.is_ascii_alphabetic())
        .map(|n| (n, s.chars().last().unwrap_or(' ')))
        .ok_or_else(|| format!("duration must end with s/m/h/d (got `{s}`)"))?;
    let n: i64 = num_str
        .parse()
        .map_err(|_| format!("could not parse `{num_str}` as an integer"))?;
    match unit {
        's' => Ok(Duration::seconds(n)),
        'm' => Ok(Duration::minutes(n)),
        'h' => Ok(Duration::hours(n)),
        'd' => Ok(Duration::days(n)),
        _ => Err(format!("unknown duration unit `{unit}` (use s/m/h/d)")),
    }
}

fn parse_datetime(s: &str) -> Result<time::OffsetDateTime, String> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| format!("could not parse `{s}` as RFC 3339 datetime: {e}"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        Command::Run(args) => run(args),
        Command::Preview(args) => preview::run(args),
        Command::Status(args) => status(args),
        Command::Forecast(args) => forecast(args),
    }
}

fn init_tracing(verbose: u8) {
    let default_level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("nami={default_level},warn")));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn run(args: RunArgs) -> Result<()> {
    // Only `run` needs an async runtime (timers, subprocess, signals);
    // preview/forecast/status stay sync.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(run::run(args))
}

fn status(_args: StatusArgs) -> Result<()> {
    unimplemented!("nami status: report reader lands in a later session")
}

fn forecast(_args: ForecastArgs) -> Result<()> {
    unimplemented!("nami forecast: provider integration lands in a later session")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_compiles_and_validates() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::seconds(30));
        assert_eq!(parse_duration("45m").unwrap(), Duration::minutes(45));
        assert_eq!(parse_duration("2h").unwrap(), Duration::hours(2));
        assert_eq!(parse_duration("1d").unwrap(), Duration::days(1));
        assert!(parse_duration("2x").is_err());
        assert!(parse_duration("h").is_err());
    }
}
