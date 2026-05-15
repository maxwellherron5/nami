//! Materiality threshold.
//!
//! A lower-carbon recommendation is only offered if the estimated
//! improvement over running now is large enough to matter. The full
//! comparison logic lands in a later session (Phase 0 implementation
//! item 3); this module currently provides only the default constant,
//! which the report layer needs to record what threshold was in effect.
//!
//! See `docs/confidence-and-materiality.md` for the rationale behind the
//! default value.

/// Default materiality threshold, as a percentage improvement of the
/// selected window's estimated average intensity over running now.
///
/// A candidate window must beat run-now by at least this percentage
/// before the scheduler will recommend deferring to it. Conservative by
/// design: forecast variance frequently exceeds this, and average
/// intensity is not marginal emissions.
pub const DEFAULT_MATERIALITY_THRESHOLD_PCT: f64 = 5.0;
