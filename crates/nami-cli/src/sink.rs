//! Report sink: writes the auditable JSON [`RunReport`] to a file
//! (`--report <path>`). The human-readable terminal summary is produced
//! separately by `preview` (item 11); this sink is the machine artifact.

use std::io;
use std::path::PathBuf;

use nami_core::{RunReport, Sink};

/// Writes a pretty-printed JSON report to the given path (overwrites).
pub struct JsonFileSink(pub PathBuf);

impl Sink for JsonFileSink {
    type Error = io::Error;

    fn record(&self, report: &RunReport) -> Result<(), io::Error> {
        let json = serde_json::to_string_pretty(report).map_err(io::Error::other)?;
        std::fs::write(&self.0, json)
    }
}
