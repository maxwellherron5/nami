//! Serde helper: represent [`time::Duration`] as whole integer seconds.
//!
//! `time::Duration`'s default serde form is a `[seconds, nanos]` array,
//! which is not human-auditable. Reports must be readable by a reviewer,
//! so durations are serialized as a plain integer number of seconds.
//! Sub-second precision is intentionally dropped — every duration `nami`
//! records (job estimates, lags, wall time) is meaningful only at second
//! resolution or coarser.
//!
//! Use via `#[serde(with = "crate::duration_secs")]` on a `Duration`
//! field, or `#[serde(with = "crate::duration_secs::option")]` on an
//! `Option<Duration>` field.

use serde::{Deserialize, Deserializer, Serializer};
use time::Duration;

pub(crate) fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_i64(d.whole_seconds())
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    Ok(Duration::seconds(i64::deserialize(d)?))
}

/// Variant for `Option<Duration>` fields.
pub(crate) mod option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use time::Duration;

    pub(crate) fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        d.map(|x| x.whole_seconds()).serialize(s)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Duration>, D::Error> {
        Ok(Option::<i64>::deserialize(d)?.map(Duration::seconds))
    }
}
