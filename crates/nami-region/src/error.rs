//! Errors emitted by `nami-region`.

use thiserror::Error;

/// A region-detection error.
#[derive(Debug, Error)]
pub enum Error {
    /// HTTP transport failure when calling the IP-geolocation service.
    #[error("http transport: {0}")]
    Http(#[from] reqwest::Error),

    /// The geolocation service responded but we could not map its answer to
    /// a supported region.
    #[error("could not map geolocation to a supported region: {0}")]
    Unmappable(String),
}

/// Result alias for region detection.
pub type Result<T> = std::result::Result<T, Error>;
