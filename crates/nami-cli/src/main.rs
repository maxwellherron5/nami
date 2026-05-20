//! The `nami` command-line entry point.
//!
//! All five subcommands are implemented: `run` (subprocess wrapping with
//! signal forwarding and exit-code propagation), `preview`, `refresh`,
//! `forecast`, and `status`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use time::Duration;
use tracing_subscriber::EnvFilter;

use nami_core::Region;

mod deadline;
mod doctor;
mod forecast;
mod init;
mod preview;
mod profile;
mod run;
mod sink;
mod status;

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

    /// Refresh one region's slice of the local historical cache from
    /// EIA-930 (requires `EIA_API_KEY`). Other regions are preserved.
    Refresh(RefreshArgs),

    /// Write a minimal nami config file with a default region (and a
    /// commented example profile), then print a brief diagnostic of
    /// what else is needed before scheduling can produce decisions.
    Init(InitArgs),

    /// Run preflight precondition checks (region resolves, eGRID
    /// table loads, EIA_API_KEY set, cache fresh) with pass/warn/fail
    /// tagging and a suggested fix per failing check. Exits nonzero
    /// on any failure (and with `--strict`, on warnings too).
    Doctor(DoctorArgs),
}

/// Args for `nami run` and `nami preview`.
#[derive(Debug, clap::Args)]
struct RunArgs {
    /// Named profile from the nami config file to source defaults from.
    /// Anything supplied on the CLI overrides the profile.
    #[arg(long)]
    pub(crate) profile: Option<String>,

    /// How long the job is expected to take, e.g. `2h`, `90m`, `45s`.
    /// Required unless `--profile` supplies a value.
    #[arg(long, value_parser = parse_duration, required_unless_present = "profile")]
    pub(crate) duration: Option<Duration>,

    /// Latest UTC instant the job is allowed to *finish*, RFC 3339 format.
    /// Required unless `--profile`, `--within`, or `--by` supplies one.
    #[arg(
        long,
        value_parser = parse_datetime,
        required_unless_present_any = ["profile", "within", "by"],
        conflicts_with_all = ["within", "by"],
    )]
    pub(crate) deadline: Option<time::OffsetDateTime>,

    /// Deadline as a duration from now: `--within 8h`, `--within 90m`.
    /// Alternative to `--deadline` / `--by`. Echoed back as the resolved
    /// UTC instant on stderr.
    #[arg(
        long,
        value_parser = parse_duration,
        conflicts_with_all = ["deadline", "by"],
    )]
    pub(crate) within: Option<Duration>,

    /// Deadline as a next-occurrence time-of-day **interpreted as UTC**:
    /// `--by 7am`, `--by 19:30`, `--by tomorrow-9am`. UTC is used
    /// deliberately (reading the host timezone is unsound under the
    /// multi-threaded runtime); use `--deadline` with an RFC 3339
    /// offset for non-UTC interpretations. Alternative to `--deadline`
    /// / `--within`. Echoed back as the resolved UTC instant on stderr.
    #[arg(
        long,
        value_parser = deadline::parse_by,
        conflicts_with_all = ["deadline", "within"],
    )]
    pub(crate) by: Option<deadline::ByTime>,

    /// Grid region (one of: CAISO, ERCOT, MISO, PJM, NYISO, ISONE, SPP).
    /// If omitted: `--profile`'s region, then `NAMI_REGION`, then the
    /// `region` key in the nami config file.
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
    /// Required unless `--profile` supplies a `command`.
    #[arg(last = true, required_unless_present = "profile")]
    pub(crate) command: Vec<String>,
}

/// Args for `nami status`.
#[derive(Debug, clap::Args)]
struct StatusArgs {
    /// Optional path to a previously written run report; if provided, also
    /// summarize that report's decision and provenance.
    #[arg(long)]
    report: Option<std::path::PathBuf>,

    /// Historical cache file to inspect.
    #[arg(long, default_value = nami_carbon_eia::DEFAULT_CACHE_PATH)]
    cache: std::path::PathBuf,

    /// eGRID factor table to check.
    #[arg(long, default_value = nami_carbon_eia::DEFAULT_EGRID_PATH)]
    egrid: std::path::PathBuf,
}

/// Args for `nami forecast`.
#[derive(Debug, clap::Args)]
struct ForecastArgs {
    /// Grid region to query. If omitted: `NAMI_REGION`, then the nami
    /// config file's `region`.
    #[arg(long)]
    region: Option<Region>,

    /// Forecast horizon, e.g. `24h`. Defaults to 24h.
    #[arg(long, value_parser = parse_duration, default_value = "24h")]
    horizon: Duration,

    /// Historical cache file to forecast from.
    #[arg(long, default_value = nami_carbon_eia::DEFAULT_CACHE_PATH)]
    cache: std::path::PathBuf,

    /// Look-back window, in weeks, for the historical-pattern model.
    #[arg(long, default_value_t = nami_carbon_eia::DEFAULT_FORECAST_WEEKS)]
    weeks: u32,
}

/// Args for `nami doctor`.
#[derive(Debug, clap::Args)]
struct DoctorArgs {
    /// Region to check. If omitted, uses the same resolution chain as
    /// `nami run` (NAMI_REGION env, then the nami config file).
    #[arg(long)]
    region: Option<Region>,

    /// Historical cache file to inspect.
    #[arg(long, default_value = nami_carbon_eia::DEFAULT_CACHE_PATH)]
    cache: std::path::PathBuf,

    /// eGRID factor table to check.
    #[arg(long, default_value = nami_carbon_eia::DEFAULT_EGRID_PATH)]
    egrid: std::path::PathBuf,

    /// Exit nonzero on warnings too (default: only on failures).
    #[arg(long, default_value_t = false)]
    strict: bool,
}

/// Args for `nami init`.
#[derive(Debug, clap::Args)]
struct InitArgs {
    /// Default grid region to record in the config file.
    #[arg(long)]
    region: Region,

    /// Path to write to. Default: the nami config path (see `--help` of
    /// `nami status` or the README) — `$NAMI_CONFIG` / `$XDG_CONFIG_HOME/
    /// nami/config.toml` / `$HOME/.config/nami/config.toml`.
    #[arg(long)]
    config: Option<std::path::PathBuf>,

    /// Overwrite an existing config file. Without this, `nami init`
    /// refuses to clobber an existing file and asks you to edit it.
    #[arg(long, default_value_t = false)]
    force: bool,

    /// Print what would be written without touching the filesystem.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

/// Args for `nami refresh`.
#[derive(Debug, clap::Args)]
struct RefreshArgs {
    /// Grid region whose cache slice to refresh from EIA-930. If omitted:
    /// `NAMI_REGION`, then the nami config file's `region`.
    #[arg(long)]
    region: Option<Region>,

    /// Weeks of hourly history to fetch (ending now, UTC).
    #[arg(long, default_value_t = nami_carbon_eia::DEFAULT_FORECAST_WEEKS)]
    weeks: u32,

    /// Historical cache file to update.
    #[arg(long, default_value = nami_carbon_eia::DEFAULT_CACHE_PATH)]
    cache: std::path::PathBuf,

    /// eGRID factor table to derive intensity with.
    #[arg(long, default_value = nami_carbon_eia::DEFAULT_EGRID_PATH)]
    egrid: std::path::PathBuf,
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

/// Resolve the region via the precedence chain (flag > `NAMI_REGION` >
/// config file > refuse). When it came from anywhere other than the
/// explicit flag, announce the source on stderr so the resolution is
/// never silent (CLAUDE.md: do not hide how a value was chosen).
pub(crate) fn resolve_region(flag: Option<Region>) -> Result<Region> {
    let resolved = nami_region::resolve_default(flag).map_err(|e| {
        anyhow::anyhow!("{e}\nsupported regions: CAISO, ERCOT, MISO, PJM, NYISO, ISONE, SPP")
    })?;
    match &resolved.source {
        nami_region::RegionSource::Flag => {}
        nami_region::RegionSource::Env => {
            eprintln!(
                "nami: region {} resolved from NAMI_REGION (no --region given)",
                resolved.region
            );
        }
        nami_region::RegionSource::Config(path) => {
            eprintln!(
                "nami: region {} resolved from config {} (no --region given)",
                resolved.region,
                path.display()
            );
        }
    }
    Ok(resolved.region)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.command {
        Command::Run(args) => run(args),
        Command::Preview(args) => preview::run(args),
        Command::Status(args) => status::run(args),
        Command::Forecast(args) => forecast::run(args),
        Command::Refresh(args) => refresh(args),
        Command::Init(args) => init::run(args),
        Command::Doctor(args) => doctor::run(args),
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

fn refresh(args: RefreshArgs) -> Result<()> {
    let region = resolve_region(args.region)?;
    // Networked: needs an async runtime, like `run`.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let now = time::OffsetDateTime::now_utc();
    let summary = rt.block_on(nami_carbon_eia::refresh_region_cache(
        region,
        args.weeks,
        &args.cache,
        &args.egrid,
        now,
    ))?;

    println!(
        "nami refresh — region {} — window {} .. {} UTC",
        summary.region,
        fmt_hour(summary.start),
        fmt_hour(summary.end),
    );
    println!(
        "Parsed {} hourly fuel-mix rows; wrote {} estimated average-intensity \
         observations; skipped {} hours with no positive generation (gaps, not \
         zeros).",
        summary.hours_parsed, summary.observations_written, summary.hours_skipped,
    );
    println!("Cache updated: {}", args.cache.display());
    for w in &summary.warnings {
        println!("Warning: {w}");
    }
    Ok(())
}

fn fmt_hour(dt: time::OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| format!("{dt:?}"))
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
