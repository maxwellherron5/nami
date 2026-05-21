//! Report sink: writes the auditable JSON [`RunReport`] to a file
//! (`--report <path>` or the auto-archive destination under
//! `--report-dir` / the default state directory). The human-readable
//! terminal summary is produced separately by `preview`/`run`; this
//! sink is the machine artifact.
//!
//! Writes go through a temp-file + rename so a crash mid-write can't
//! leave a half-written report in place — important once reports are
//! auto-archived (a corrupt file in the state dir would poison every
//! future `nami report summary` aggregation).

use std::io;
use std::path::{Path, PathBuf};

use nami_core::{RunReport, Sink};

/// Writes a pretty-printed JSON report to the given path. Creates the
/// parent directory if needed and writes atomically (temp file +
/// rename).
pub struct JsonFileSink(pub PathBuf);

impl Sink for JsonFileSink {
    type Error = io::Error;

    fn record(&self, report: &RunReport) -> Result<(), io::Error> {
        let json = serde_json::to_string_pretty(report).map_err(io::Error::other)?;
        write_atomic(&self.0, json.as_bytes())
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "report path has no filename"))?
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
