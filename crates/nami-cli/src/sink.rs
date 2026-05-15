//! Report sinks: where a [`RunReport`] is written.
//!
//! Phase 0 ships two: pretty-printed JSON to stdout (the default) and to
//! a file (`--report <path>`). The JSON is the auditable artifact;
//! human-friendly summary formatting is a later session (CLI preview
//! output, item 11).

use std::io::{self, Write};
use std::path::PathBuf;

use nami_core::{RunReport, Sink};

/// A destination for the run report.
pub enum ReportSink {
    /// Pretty JSON to stdout.
    Stdout,
    /// Pretty JSON to the given file path (overwrites).
    File(PathBuf),
}

impl Sink for ReportSink {
    type Error = io::Error;

    fn record(&self, report: &RunReport) -> Result<(), io::Error> {
        let json = serde_json::to_string_pretty(report).map_err(io::Error::other)?;
        match self {
            ReportSink::Stdout => {
                let mut out = io::stdout().lock();
                out.write_all(json.as_bytes())?;
                out.write_all(b"\n")?;
                out.flush()
            }
            ReportSink::File(path) => std::fs::write(path, json),
        }
    }
}
