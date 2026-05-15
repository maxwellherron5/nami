//! Offline static-fallback "provider" for `nami`.
//!
//! Provides one number per region: a coarse annual mean. See
//! [`StaticTableProvider`] for the design rationale on why this is
//! intentionally **not** a forecast or historical provider.

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

mod error;
mod provider;
mod table;

pub use error::{Error, Result};
pub use provider::StaticTableProvider;
