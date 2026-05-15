//! U.S. balancing-authority regions supported by `nami`.
//!
//! Regions are an explicit enum, not a free-form string: adding a new region
//! is a deliberate code change so that we never silently route a job to a
//! region we have no carbon data for. Phase 0 supports the seven major
//! U.S. ISO/RTO balancing authorities covered by EIA-930.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};

/// A U.S. balancing-authority grid region.
///
/// Codes correspond to EIA-930 BA identifiers. Non-U.S. regions are out of
/// scope until Phase 1 adds ENTSO-E or the UK Carbon Intensity API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Region {
    /// California ISO.
    Caiso,
    /// Electric Reliability Council of Texas.
    Ercot,
    /// Midcontinent ISO.
    Miso,
    /// PJM Interconnection.
    Pjm,
    /// New York ISO.
    Nyiso,
    /// ISO New England.
    IsoNe,
    /// Southwest Power Pool.
    Spp,
}

impl Region {
    /// The canonical EIA-930 BA code for this region.
    pub const fn as_code(self) -> &'static str {
        match self {
            Region::Caiso => "CAISO",
            Region::Ercot => "ERCOT",
            Region::Miso => "MISO",
            Region::Pjm => "PJM",
            Region::Nyiso => "NYISO",
            Region::IsoNe => "ISONE",
            Region::Spp => "SPP",
        }
    }

    /// All regions supported by this build.
    pub const ALL: &'static [Region] = &[
        Region::Caiso,
        Region::Ercot,
        Region::Miso,
        Region::Pjm,
        Region::Nyiso,
        Region::IsoNe,
        Region::Spp,
    ];
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for Region {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_uppercase().as_str() {
            "CAISO" => Ok(Region::Caiso),
            "ERCOT" => Ok(Region::Ercot),
            "MISO" => Ok(Region::Miso),
            "PJM" => Ok(Region::Pjm),
            "NYISO" => Ok(Region::Nyiso),
            "ISONE" | "ISO_NE" | "ISO-NE" => Ok(Region::IsoNe),
            "SPP" => Ok(Region::Spp),
            other => Err(Error::UnknownRegion(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_codes() {
        for region in Region::ALL {
            assert_eq!(Region::from_str(region.as_code()).unwrap(), *region);
        }
    }

    #[test]
    fn parse_is_case_insensitive() {
        assert_eq!(Region::from_str("ercot").unwrap(), Region::Ercot);
        assert_eq!(Region::from_str("Iso-NE").unwrap(), Region::IsoNe);
    }

    #[test]
    fn rejects_unknown() {
        assert!(Region::from_str("ATLANTIS").is_err());
    }
}
