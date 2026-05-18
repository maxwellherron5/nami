//! Scheduling algorithms and policies for `nami`.
//!
//! Phase 0 status: the fallback policy ([`static_fallback_decision`]),
//! the materiality logic ([`assess_materiality`]), candidate-window
//! generation ([`candidate_windows`]), and the windowed
//! [`BestWindowScheduler`] are implemented.

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

mod best_window;
mod error;
mod fallback;
mod materiality;
mod window;

pub use best_window::{BestWindowScheduler, WindowScore, score_window};
pub use error::{Error, Result};
pub use fallback::static_fallback_decision;
pub use materiality::{DEFAULT_MATERIALITY_THRESHOLD_PCT, MaterialityVerdict, assess_materiality};
pub use window::{CandidateWindow, candidate_windows};
