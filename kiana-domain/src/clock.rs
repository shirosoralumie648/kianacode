//! Trusted wall/monotonic clock observations for expiry and scheduling boundaries.
//!
//! A clock value is an input snapshot, not authority.  A rollback or untrusted observation may
//! be persisted for diagnosis, but it must never be used to extend a lease, approval or trigger.
//! The contract is deliberately synchronous and side-effect free; adapters own the actual OS or
//! fake-clock sampling.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CLOCK_OBSERVATION_SCHEMA: &str = "kiana.clock-observation.v1";
pub const CLOCK_OBSERVATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockTrust {
    Trusted,
    Untrusted,
    Rollback,
    Overflow,
}

impl ClockTrust {
    pub fn is_trusted(self) -> bool {
        self == Self::Trusted
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockObservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub source: String,
    pub wall_now_unix_ms: u64,
    pub monotonic_now_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_wall_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_monotonic_now_ms: Option<u64>,
    pub revision: u64,
    pub trust: ClockTrust,
    pub observation_digest: String,
}

impl ClockObservation {
    /// Build an observation from bounded adapter samples.  `u128` inputs make conversion overflow
    /// explicit instead of wrapping an OS duration into a future deadline.
    pub fn observe(
        source: impl Into<String>,
        wall_now_unix_ms: u128,
        monotonic_now_ms: u128,
        previous: Option<&Self>,
        revision: u64,
    ) -> Result<Self, String> {
        if wall_now_unix_ms == 0 || monotonic_now_ms == 0 {
            return Err("clock_zero_sample".to_owned());
        }
        let wall_now_unix_ms =
            u64::try_from(wall_now_unix_ms).map_err(|_| "clock_wall_overflow".to_owned())?;
        let monotonic_now_ms =
            u64::try_from(monotonic_now_ms).map_err(|_| "clock_monotonic_overflow".to_owned())?;
        let source = source.into();
        if source.trim().is_empty() || source.len() > 128 || source.contains(['\0', '\r', '\n']) {
            return Err("clock_source_invalid".to_owned());
        }
        if revision == 0 || previous.is_some_and(|prior| revision <= prior.revision) {
            return Err("clock_revision_invalid".to_owned());
        }
        let trust = if previous.is_some_and(|prior| {
            wall_now_unix_ms < prior.wall_now_unix_ms || monotonic_now_ms < prior.monotonic_now_ms
        }) {
            ClockTrust::Rollback
        } else {
            ClockTrust::Trusted
        };
        let mut observation = Self {
            schema: CLOCK_OBSERVATION_SCHEMA.to_owned(),
            version: CLOCK_OBSERVATION_VERSION,
            source,
            wall_now_unix_ms,
            monotonic_now_ms,
            previous_wall_unix_ms: previous.map(|prior| prior.wall_now_unix_ms),
            previous_monotonic_now_ms: previous.map(|prior| prior.monotonic_now_ms),
            revision,
            trust,
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CLOCK_OBSERVATION_SCHEMA
            || !self.version.is_compatible_with(&CLOCK_OBSERVATION_VERSION)
            || self.source.trim().is_empty()
            || self.source.len() > 128
            || self.source.contains(['\0', '\r', '\n'])
            || self.wall_now_unix_ms == 0
            || self.monotonic_now_ms == 0
            || self.revision == 0
            || !self.observation_digest.starts_with("sha256:")
            || self.observation_digest.len() != 71
            || self.observation_digest != self.digest()
        {
            return Err("clock_observation_invalid".to_owned());
        }
        if self
            .previous_wall_unix_ms
            .is_some_and(|previous| previous == 0)
            || self
                .previous_monotonic_now_ms
                .is_some_and(|previous| previous == 0)
        {
            return Err("clock_previous_sample_invalid".to_owned());
        }
        if self.trust == ClockTrust::Trusted
            && (self
                .previous_wall_unix_ms
                .is_some_and(|previous| self.wall_now_unix_ms < previous)
                || self
                    .previous_monotonic_now_ms
                    .is_some_and(|previous| self.monotonic_now_ms < previous))
        {
            return Err("clock_trust_rollback_mismatch".to_owned());
        }
        Ok(())
    }

    /// Validate a persisted observation against a newer process snapshot.
    pub fn validate_transition_from(&self, previous: &Self) -> Result<(), String> {
        self.validate()?;
        previous.validate()?;
        if self.source != previous.source || self.revision <= previous.revision {
            return Err("clock_transition_revision_invalid".to_owned());
        }
        let rollback = self.wall_now_unix_ms < previous.wall_now_unix_ms
            || self.monotonic_now_ms < previous.monotonic_now_ms;
        if rollback && self.trust == ClockTrust::Trusted {
            return Err("clock_transition_rollback_untrusted".to_owned());
        }
        if !rollback && self.trust == ClockTrust::Rollback {
            return Err("clock_transition_rollback_stale".to_owned());
        }
        Ok(())
    }

    pub fn require_trusted(&self) -> Result<(), String> {
        self.validate()?;
        if self.trust.is_trusted() {
            Ok(())
        } else {
            Err("clock_untrusted".to_owned())
        }
    }

    /// Check an existing expiry without ever extending it.  Untrusted time is a hard deny.
    pub fn allows_before(&self, expires_at_unix_ms: u64) -> Result<bool, String> {
        self.require_trusted()?;
        if expires_at_unix_ms == 0 {
            return Err("clock_deadline_invalid".to_owned());
        }
        Ok(self.wall_now_unix_ms < expires_at_unix_ms)
    }

    /// Clamp a candidate deadline to a trusted cap.  A rollback cannot turn a stale lease into a
    /// fresh one because this helper refuses to produce any deadline at all.
    pub fn clamp_deadline(&self, candidate: u64, cap: u64) -> Result<u64, String> {
        self.require_trusted()?;
        if candidate == 0 || cap == 0 {
            return Err("clock_deadline_invalid".to_owned());
        }
        let deadline = candidate.min(cap);
        if deadline <= self.wall_now_unix_ms {
            return Err("clock_deadline_expired".to_owned());
        }
        Ok(deadline)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source": self.source,
            "wall_now_unix_ms": self.wall_now_unix_ms,
            "monotonic_now_ms": self.monotonic_now_ms,
            "previous_wall_unix_ms": self.previous_wall_unix_ms,
            "previous_monotonic_now_ms": self.previous_monotonic_now_ms,
            "revision": self.revision,
            "trust": self.trust,
        }))
    }
}
