//! Errors returned by the EIA-930 + eGRID provider.

use thiserror::Error;

/// An EIA provider error.
#[derive(Debug, Error)]
pub enum Error {
    /// HTTP transport failure when calling the EIA v2 API.
    #[error("http transport: {0}")]
    Http(#[from] reqwest::Error),

    /// The EIA API returned a non-success status with a body.
    #[error("eia api {status}: {body}")]
    Api {
        /// HTTP status code returned by EIA.
        status: u16,
        /// Response body, truncated for log safety.
        body: String,
    },

    /// `EIA_API_KEY` environment variable was required but absent.
    #[error("EIA_API_KEY environment variable is not set")]
    MissingApiKey,

    /// The local emission-factor table (eGRID) was missing or malformed.
    #[error("egrid factor table: {0}")]
    EgridTable(String),

    /// The historical cache file does not exist at the given path. This is
    /// a *state*, not necessarily a fatal error: the caller typically maps
    /// it to a static-fallback `DataFreshness`.
    #[error("historical cache not found: {0}")]
    CacheMissing(String),

    /// The cache file exists but its `schema_version` is not one this
    /// build understands. We refuse to interpret an unknown format rather
    /// than silently misread it.
    #[error("historical cache schema mismatch: found v{found}, expected v{expected}")]
    CacheSchemaMismatch {
        /// Schema version found in the file.
        found: u32,
        /// Schema version this build supports.
        expected: u32,
    },

    /// The cache file exists and parsed, but failed structural validation
    /// (e.g., observations not strictly time-ordered, duplicate region).
    #[error("historical cache: {0}")]
    HistoricalCache(String),

    /// Filesystem I/O failure reading or writing a local file.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// A value returned by EIA failed validation or violated invariants.
    #[error("eia returned malformed data: {0}")]
    Malformed(String),

    /// Carbon intensity could not be derived for a given hour (e.g. no
    /// positive generation after clamping). Not a bug — the caller treats
    /// the hour as a gap rather than inventing a number.
    #[error("could not derive intensity: {0}")]
    DerivationFailed(String),
}

/// Result alias for the EIA provider.
pub type Result<T> = std::result::Result<T, Error>;
