//! Offline [`CarbonProvider`](nami_core::CarbonProvider) backed by a static
//! table of annual-average regional carbon intensities.
//!
//! See [`StaticTableProvider`] for details and design rationale.

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

mod error;
mod provider;
mod table;

pub use error::{Error, Result};
pub use provider::StaticTableProvider;
