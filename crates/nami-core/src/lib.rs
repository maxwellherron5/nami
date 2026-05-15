//! Core domain types and traits for [`nami`](https://github.com/maxwellherron/nami),
//! a carbon-aware scheduler for ML training jobs.
//!
//! This crate is the boundary every other `nami-*` crate depends on. It
//! intentionally contains no I/O, no async runtime, and no provider-specific
//! logic — only the types that flow across crate boundaries and the traits
//! that define the seams.
//!
//! # Layout
//!
//! - Input contract: [`JobSpec`]
//! - Domain values: [`Region`], [`CarbonIntensity`], [`IntensityKind`],
//!   [`ForecastPoint`], [`Confidence`]
//! - Scheduler output: [`SchedulingDecision`], [`StartReason`], [`RefuseReason`]
//! - Run output: [`RunReport`], [`CarbonOutcome`], [`DataGap`]
//! - Seams: [`CarbonProvider`], [`Scheduler`], [`Sink`]
//!
//! # Conventions
//!
//! - Carbon intensities are gCO₂/kWh internally; convert at API boundaries.
//! - Times are [`time::OffsetDateTime`] in UTC; never naive local time.
//! - Errors are typed per-crate via `thiserror`; this crate's error lives in
//!   [`Error`].

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

mod carbon;
mod decision;
mod error;
mod job;
mod provider;
mod region;
mod report;
mod scheduler;
mod sink;

pub use carbon::{CarbonIntensity, Confidence, ForecastPoint, IntensityKind};
pub use decision::{RefuseReason, SchedulingDecision, StartReason};
pub use error::{Error, Result};
pub use job::JobSpec;
pub use provider::CarbonProvider;
pub use region::Region;
pub use report::{CarbonOutcome, DataGap, RunReport};
pub use scheduler::Scheduler;
pub use sink::Sink;
