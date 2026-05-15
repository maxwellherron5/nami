//! Errors emitted by `nami-core`.
//!
//! `nami-core` itself does little work — its errors are mostly produced when
//! deserializing or validating inputs. Each downstream crate exports its own
//! [`Error`] enum; do not collapse them into a single workspace error type.

use thiserror::Error;

/// A `nami-core` error.
#[derive(Debug, Error)]
pub enum Error {
    /// A region identifier was not recognized.
    #[error("unknown region: {0}")]
    UnknownRegion(String),

    /// A carbon-intensity value was negative or otherwise out of range.
    #[error("invalid carbon intensity: {0}")]
    InvalidIntensity(String),

    /// A job specification failed validation.
    #[error("invalid job spec: {0}")]
    InvalidJobSpec(String),
}

/// A `nami-core` result alias.
pub type Result<T> = std::result::Result<T, Error>;
