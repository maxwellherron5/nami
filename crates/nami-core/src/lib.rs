//! Core domain types and traits for [`nami`](https://github.com/maxwellherron/nami),
//! a conservative, uncertainty-aware, public-data carbon-aware scheduler.
//!
//! This crate is the boundary every other `nami-*` crate depends on. It
//! intentionally contains no I/O, no async runtime, and no provider-
//! specific logic — only the types that flow across crate boundaries and
//! the traits that define the seams.
//!
//! # Phase 0 commitments
//!
//! - Carbon intensity is always *estimated average*, derived from
//!   EIA-930 fuel-mix observations multiplied through EPA eGRID factors.
//!   Marginal emissions are out of scope.
//! - Confidence ([`Confidence`], [`ConfidenceLevel`], [`ConfidenceInterval`])
//!   and freshness ([`DataFreshness`]) travel with every estimate.
//! - Providers declare their capabilities via [`ProviderMetadata`] /
//!   [`ProviderCapability`]; they implement exactly the data-fetching
//!   traits they can honestly support
//!   ([`HistoricalCarbonProvider`], [`ForecastProvider`],
//!   [`RealtimeGridProvider`]).
//!
//! # Conventions
//!
//! - Carbon intensities are gCO₂/kWh internally; convert at boundaries.
//! - Times are [`time::OffsetDateTime`] in UTC; never naive local time.
//! - Errors are typed per-crate via `thiserror`; this crate's error lives
//!   in [`Error`].

#![deny(missing_docs)]
#![deny(rust_2018_idioms)]

mod carbon;
mod confidence;
mod decision;
mod error;
mod job;
mod observation;
mod provider;
mod region;
mod report;
mod scheduler;
mod sink;

pub use carbon::{CarbonIntensity, EmissionFactor, FuelType};
pub use confidence::{
    Confidence, ConfidenceInterval, ConfidenceLevel, DataFreshness, DataGranularity,
};
pub use decision::{RefuseReason, SchedulingDecision, StartReason};
pub use error::{Error, Result};
pub use job::JobSpec;
pub use observation::{CarbonObservation, ForecastHorizon, ForecastPoint};
pub use provider::{
    ForecastProvider, GridSnapshot, HistoricalCarbonProvider, ProviderCapability, ProviderInfo,
    ProviderMetadata, RealtimeGridProvider,
};
pub use region::Region;
pub use report::{RunReport, WindowEstimate};
pub use scheduler::Scheduler;
pub use sink::Sink;
