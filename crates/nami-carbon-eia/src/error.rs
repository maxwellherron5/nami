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

    /// The local historical cache file was missing, stale, or corrupt.
    #[error("historical cache: {0}")]
    HistoricalCache(String),

    /// A value returned by EIA failed validation or violated invariants.
    #[error("eia returned malformed data: {0}")]
    Malformed(String),
}

/// Result alias for the EIA provider.
pub type Result<T> = std::result::Result<T, Error>;
