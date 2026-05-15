//! Grid regions in which a job may run.
//!
//! Regions are an explicit enum, not a free-form string: adding a new region
//! is a deliberate code change so that we never silently route a job to a
//! region we have no carbon data for. Phase 0 supports the US ISO/RTO
//! balancing authorities that WattTime's free tier covers.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};

/// A grid region.
///
/// US regions use WattTime balancing-authority codes (`CAISO_NORTH`, `ERCOT`,
/// etc.). Non-US coverage will arrive in Phase 1 and is intentionally absent
/// here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Region {
    /// California ISO, northern subregion.
    CaisoNorth,
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
    /// The canonical WattTime BA code for this region.
    pub const fn as_code(self) -> &'static str {
        match self {
            Region::CaisoNorth => "CAISO_NORTH",
            Region::Ercot => "ERCOT",
            Region::Miso => "MISO",
            Region::Pjm => "PJM",
            Region::Nyiso => "NYISO",
            Region::IsoNe => "ISONE",
            Region::Spp => "SPP",
        }
    }

    /// All regions known to this build.
    pub const ALL: &'static [Region] = &[
        Region::CaisoNorth,
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
            "CAISO_NORTH" | "CAISO-NORTH" => Ok(Region::CaisoNorth),
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
