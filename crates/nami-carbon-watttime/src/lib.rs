//! WattTime [`CarbonProvider`](nami_core::CarbonProvider) implementation.
//!
//! Phase 0 stub. The HTTP client, token caching, and forecast / signal-index
//! parsing land in a subsequent session — this skeleton only exposes the
//! crate's error type so the workspace compiles.

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

mod error;

pub use error::{Error, Result};
