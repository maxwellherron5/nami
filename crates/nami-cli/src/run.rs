//! `nami run`: compute a recommendation, wait for the chosen window, then
//! wrap the user's command as a child process.
//!
//! The decision and [`RunReport`] are produced by the same code path as
//! `nami preview` ([`crate::preview::assemble`]); this module adds the
//! side-effecting parts CLAUDE.md calls out as correctness concerns:
//!
//! - the program is resolved **before** scheduling, so a typo fails fast
//!   instead of after a multi-hour wait;
//! - the wait phase is interruptible — SIGINT before the child starts
//!   cancels the schedule cleanly (exit 0), nothing is spawned;
//! - during the run, SIGINT/SIGTERM/SIGHUP are forwarded to the child,
//!   then SIGKILL after a grace period if it has not exited;
//! - the child's exit code is propagated as `nami`'s exit code.

use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use anyhow::{Context, Result, anyhow};
use time::{Duration, OffsetDateTime};
use tokio::process::Command;
use tokio::signal::unix::{SignalKind, signal};

use nami_carbon_eia::DEFAULT_CACHE_PATH;
use nami_core::{RunReport, SchedulingDecision, Sink};

use crate::RunArgs;
use crate::preview::{assemble, human_summary, load_cache};
use crate::sink::JsonFileSink;

/// How long the child gets to exit on its own after a forwarded
/// termination signal before `nami` escalates to SIGKILL.
const GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(10);

/// Run `nami run`. Never returns `Ok` on the normal path: it exits the
/// process with the child's exit code (or 0 on a cancelled wait, or 1 on
/// a refusal). It returns `Err` only for setup failures before scheduling.
pub async fn run(args: RunArgs) -> Result<()> {
    let (program, _) = args
        .command
        .split_first()
        .ok_or_else(|| anyhow!("empty command"))?;
    resolve_program(program, std::env::var("PATH").ok().as_deref())
        .with_context(|| format!("cannot schedule `{program}`"))?;

    let now = OffsetDateTime::now_utc();
    let cache = load_cache(DEFAULT_CACHE_PATH, now);
    let mut report = assemble(&args, now, cache)?;

    print!("{}", human_summary(&report));
    flush_stdout();

    let start_time = match &report.decision {
        SchedulingDecision::Refuse { .. } => {
            finalize(&report, &args);
            std::process::exit(1);
        }
        SchedulingDecision::StartImmediately { .. } => OffsetDateTime::now_utc(),
        SchedulingDecision::StartAt { start_time, .. } => *start_time,
    };

    if !wait_until(start_time).await {
        report.warnings.push(
            "Schedule cancelled by SIGINT during the wait phase; the command was not started."
                .to_string(),
        );
        eprintln!("\nnami: schedule cancelled (SIGINT during wait); command not started.");
        finalize(&report, &args);
        std::process::exit(0);
    }

    let started_at = OffsetDateTime::now_utc();
    let mut child = spawn_child(&args).context("failed to spawn the wrapped command")?;
    let pid = child.id().expect("child has a PID before it is awaited") as libc::pid_t;

    let status = supervise(&mut child, pid).await?;

    let finished_at = OffsetDateTime::now_utc();
    report.started_at = Some(started_at);
    report.finished_at = Some(finished_at);
    report.wall_duration = Some(finished_at - started_at);
    let code = exit_code(&status);
    report.exit_code = Some(code);
    finalize(&report, &args);
    std::process::exit(code);
}

/// Sleep until `start_time`, interruptible by SIGINT. Returns `true` if
/// the wait completed normally, `false` if SIGINT cancelled it.
async fn wait_until(start_time: OffsetDateTime) -> bool {
    let remaining = start_time - OffsetDateTime::now_utc();
    if remaining <= Duration::ZERO {
        return true;
    }
    let sleep = tokio::time::sleep(remaining.unsigned_abs());
    tokio::select! {
        _ = sleep => true,
        _ = tokio::signal::ctrl_c() => false,
    }
}

/// Build the child `Command` with stdio wired per `--quiet` / `--log`.
fn spawn_child(args: &RunArgs) -> Result<tokio::process::Child> {
    let (program, rest) = args
        .command
        .split_first()
        .ok_or_else(|| anyhow!("empty command"))?;

    let mut cmd = Command::new(program);
    cmd.args(rest).kill_on_drop(true);

    if let Some(log) = &args.log {
        let file = std::fs::File::create(log)
            .with_context(|| format!("cannot open log file {}", log.display()))?;
        let err = file
            .try_clone()
            .context("cannot duplicate log file handle")?;
        cmd.stdout(Stdio::from(file)).stderr(Stdio::from(err));
    } else if args.quiet {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    } else {
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }

    cmd.spawn().map_err(Into::into)
}

/// Wait for the child while forwarding terminating signals to it. After
/// the first forwarded signal the child has [`GRACE_PERIOD`] to exit
/// before `nami` escalates to SIGKILL.
async fn supervise(child: &mut tokio::process::Child, pid: libc::pid_t) -> Result<ExitStatus> {
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sighup = signal(SignalKind::hangup())?;
    let mut force_kill_at: Option<tokio::time::Instant> = None;

    loop {
        tokio::select! {
            status = child.wait() => return status.map_err(Into::into),
            _ = sigint.recv() => forward(pid, libc::SIGINT, &mut force_kill_at),
            _ = sigterm.recv() => forward(pid, libc::SIGTERM, &mut force_kill_at),
            _ = sighup.recv() => forward(pid, libc::SIGHUP, &mut force_kill_at),
            _ = grace_elapsed(force_kill_at) => {
                eprintln!("nami: grace period elapsed; sending SIGKILL to child {pid}.");
                // SAFETY: kill(2) with a pid we own; well-defined.
                unsafe { libc::kill(pid, libc::SIGKILL); }
                force_kill_at = None; // don't busy-loop on the elapsed deadline
            }
        }
    }
}

/// Forward `sig` to the child and arm the SIGKILL grace deadline on the
/// first terminating signal.
fn forward(pid: libc::pid_t, sig: libc::c_int, force_kill_at: &mut Option<tokio::time::Instant>) {
    eprintln!("nami: forwarding signal {sig} to child {pid}.");
    // SAFETY: kill(2) with a pid we own; well-defined.
    unsafe {
        libc::kill(pid, sig);
    }
    if force_kill_at.is_none() {
        *force_kill_at = Some(tokio::time::Instant::now() + GRACE_PERIOD);
    }
}

/// Resolves when the grace deadline passes; never resolves if unset.
async fn grace_elapsed(force_kill_at: Option<tokio::time::Instant>) {
    match force_kill_at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Propagate the child's exit code. A child killed by a signal has no
/// code; follow the shell convention of `128 + signal`.
fn exit_code(status: &ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

/// Write the report to `--report` if given, otherwise print it (pretty
/// JSON) to stdout so the run still leaves an auditable artifact.
fn finalize(report: &RunReport, args: &RunArgs) {
    if let Some(path) = &args.report {
        if let Err(e) = JsonFileSink(path.clone()).record(report) {
            eprintln!(
                "nami: failed to write run report to {}: {e}",
                path.display()
            );
        }
    } else {
        match serde_json::to_string_pretty(report) {
            Ok(json) => println!("{json}"),
            Err(e) => eprintln!("nami: failed to serialize run report: {e}"),
        }
    }
}

fn flush_stdout() {
    let _ = std::io::stdout().flush();
}

/// Validate that `program` exists and is runnable *before* scheduling, so
/// a typo fails immediately instead of after a long wait.
///
/// A path-like `program` (contains `/`) is checked directly; a bare name
/// is searched on `path_env` (the `PATH` value). Returns `Ok` if a
/// matching regular, executable file is found.
fn resolve_program(program: &str, path_env: Option<&str>) -> Result<()> {
    if program.is_empty() {
        return Err(anyhow!("empty command"));
    }
    if program.contains('/') {
        let p = Path::new(program);
        return if is_executable_file(p) {
            Ok(())
        } else {
            Err(anyhow!("`{program}` is not an executable file"))
        };
    }
    let path = path_env.ok_or_else(|| {
        anyhow!("`{program}` is not a path and PATH is unset, so it cannot be resolved")
    })?;
    for dir in path.split(':').filter(|d| !d.is_empty()) {
        let candidate: PathBuf = Path::new(dir).join(program);
        if is_executable_file(&candidate) {
            return Ok(());
        }
    }
    Err(anyhow!("`{program}` was not found on PATH"))
}

/// True if `p` is a regular file with at least one execute bit set.
fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_absolute_path_to_real_binary() {
        // /bin/sh is a regular executable on Linux and macOS.
        assert!(resolve_program("/bin/sh", None).is_ok());
    }

    #[test]
    fn rejects_missing_absolute_path() {
        assert!(resolve_program("/no/such/binary-xyz", None).is_err());
    }

    #[test]
    fn rejects_directory_as_program() {
        assert!(resolve_program("/bin", None).is_err());
    }

    #[test]
    fn finds_bare_name_on_path() {
        // The directory containing /bin/sh, fed in as PATH.
        assert!(resolve_program("sh", Some("/usr/bin:/bin")).is_ok());
    }

    #[test]
    fn bare_name_not_on_path_fails() {
        assert!(resolve_program("definitely-not-a-real-cmd-xyz", Some("/bin")).is_err());
    }

    #[test]
    fn bare_name_without_path_env_fails() {
        assert!(resolve_program("sh", None).is_err());
    }

    #[test]
    fn signal_terminated_status_maps_to_128_plus_signal() {
        // Simulate: spawn `sh -c 'kill -TERM $$'` and read its status.
        let st = std::process::Command::new("/bin/sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .expect("spawn sh");
        assert_eq!(exit_code(&st), 128 + libc::SIGTERM);
    }

    #[test]
    fn normal_exit_code_is_propagated() {
        let st = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 42"])
            .status()
            .expect("spawn sh");
        assert_eq!(exit_code(&st), 42);
    }
}
