//! Errors emitted by `nami-region`.

use thiserror::Error;

/// A region-resolution error.
#[derive(Debug, Error)]
pub enum Error {
    /// A region string from the environment or config file was not a
    /// supported balancing authority.
    #[error("{from} value {value:?} is not a supported region")]
    InvalidRegion {
        /// The offending raw value.
        value: String,
        /// Where it came from (for an actionable message).
        from: &'static str,
    },

    /// The config file existed but could not be parsed as TOML.
    #[error("config file {path}: {msg}")]
    Config {
        /// Config file path.
        path: String,
        /// Parser message.
        msg: String,
    },

    /// Filesystem error reading the config file.
    #[error("io reading config: {0}")]
    Io(#[from] std::io::Error),

    /// No region from the flag, the environment, or the config file.
    /// `nami` refuses to guess: balancing-authority boundaries do not
    /// follow timezones or state lines, so a heuristic would be
    /// confidently wrong too often.
    #[error(
        "no region: pass --region, set NAMI_REGION, or add `region = \"<BA>\"` \
         to the nami config file"
    )]
    Unresolved,
}

/// Result alias for region resolution.
pub type Result<T> = std::result::Result<T, Error>;
