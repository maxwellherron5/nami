//! Scheduling algorithms and policies for `nami`.
//!
//! Phase 0 status: the fallback policy ([`static_fallback_decision`]) and
//! the materiality constant ([`DEFAULT_MATERIALITY_THRESHOLD_PCT`]) are
//! implemented. The windowed `BestWindowScheduler` (candidate-window
//! scoring against a real forecast) lands in a later session per
//! `CLAUDE.md`'s "Phase 0 implementation goals".

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

mod error;
mod fallback;
mod materiality;

pub use error::{Error, Result};
pub use fallback::static_fallback_decision;
pub use materiality::DEFAULT_MATERIALITY_THRESHOLD_PCT;
