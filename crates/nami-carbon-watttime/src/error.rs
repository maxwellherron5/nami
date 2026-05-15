//! Errors returned by the WattTime provider.

use thiserror::Error;

/// A WattTime client error.
#[derive(Debug, Error)]
pub enum Error {
    /// HTTP transport failure.
    #[error("http transport: {0}")]
    Http(#[from] reqwest::Error),

    /// The WattTime API returned a non-success status with a body.
    #[error("watttime api {status}: {body}")]
    Api {
        /// HTTP status code returned by WattTime.
        status: u16,
        /// Response body, truncated for log safety.
        body: String,
    },

    /// Authentication failed (bad credentials or expired token).
    #[error("watttime authentication failed")]
    Auth,

    /// Region is not supported by WattTime.
    #[error("watttime does not cover region: {0}")]
    UnsupportedRegion(String),

    /// The API returned a value we could not parse or that violated invariants.
    #[error("watttime returned malformed data: {0}")]
    Malformed(String),
}

/// Result alias for the WattTime provider.
pub type Result<T> = std::result::Result<T, Error>;
