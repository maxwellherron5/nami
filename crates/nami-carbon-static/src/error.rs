//! Errors emitted by the static-table fallback provider.

use thiserror::Error;

use nami_core::Region;

/// A static-table provider error.
#[derive(Debug, Error)]
pub enum Error {
    /// The region was not present in the static fallback table.
    #[error("static fallback table has no entry for region: {0}")]
    UnsupportedRegion(Region),

    /// A table entry violated `CarbonIntensity` invariants. This is a
    /// build-time bug in the table, not a runtime failure mode.
    #[error("static table value is invalid: {0}")]
    BadTableValue(#[from] nami_core::Error),
}

/// Result alias for the static-table provider.
pub type Result<T> = std::result::Result<T, Error>;
