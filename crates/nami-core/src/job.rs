//! User-supplied job specification.
//!
//! A [`JobSpec`] is the input contract to the scheduler: a command to wrap,
//! an estimated duration, a hard deadline, and a region. Everything else
//! (which provider to use, where reports go, etc.) is wired in by the CLI.

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::error::{Error, Result};
use crate::region::Region;

/// What the user wants to run, plus the temporal flexibility they're offering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSpec {
    /// The command and arguments to execute. `command[0]` is the program;
    /// the remainder are passed as positional arguments. Not interpreted by
    /// a shell.
    pub command: Vec<String>,

    /// The user's estimate of how long the job runs. Used to size the
    /// scheduling window.
    pub estimated_duration: Duration,

    /// The latest moment the job is permitted to *finish*. If the cleanest
    /// window plus `estimated_duration` ends after this instant, the
    /// scheduler must shorten its search or refuse.
    pub deadline: OffsetDateTime,

    /// The grid region in which the job will physically run.
    pub region: Region,
}

impl JobSpec {
    /// Validate basic invariants: non-empty command, positive duration,
    /// deadline strictly in the future relative to `now`, and enough time
    /// between `now` and `deadline` to fit `estimated_duration`.
    pub fn validate(&self, now: OffsetDateTime) -> Result<()> {
        if self.command.is_empty() {
            return Err(Error::InvalidJobSpec("command must be non-empty".into()));
        }
        if self.estimated_duration <= Duration::ZERO {
            return Err(Error::InvalidJobSpec(
                "estimated_duration must be positive".into(),
            ));
        }
        if self.deadline <= now {
            return Err(Error::InvalidJobSpec(
                "deadline must be in the future".into(),
            ));
        }
        if self.deadline - now < self.estimated_duration {
            return Err(Error::InvalidJobSpec(
                "deadline does not leave room for estimated_duration".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn spec() -> JobSpec {
        JobSpec {
            command: vec!["python".into(), "train.py".into()],
            estimated_duration: Duration::hours(2),
            deadline: datetime!(2030-01-01 12:00 UTC),
            region: Region::Ercot,
        }
    }

    #[test]
    fn validates_a_reasonable_spec() {
        let now = datetime!(2030-01-01 06:00 UTC);
        assert!(spec().validate(now).is_ok());
    }

    #[test]
    fn rejects_empty_command() {
        let mut s = spec();
        s.command.clear();
        assert!(s.validate(datetime!(2030-01-01 06:00 UTC)).is_err());
    }

    #[test]
    fn rejects_past_deadline() {
        let now = datetime!(2030-01-01 13:00 UTC);
        assert!(spec().validate(now).is_err());
    }

    #[test]
    fn rejects_too_tight_deadline() {
        let now = datetime!(2030-01-01 11:00 UTC); // 1h left, need 2h
        assert!(spec().validate(now).is_err());
    }
}
