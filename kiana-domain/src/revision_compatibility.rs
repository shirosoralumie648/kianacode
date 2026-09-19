//! Build/replay revision pins and old-revision drain contract.
//!
//! A revision pin is an immutable input snapshot for a run. It is not a provider request and
//! cannot switch a running run. Drain is a pure gate; the durable lease/fence and actual routing
//! remain outside this domain contract.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const REVISION_PIN_SCHEMA: &str = "kiana.execution-revision-pin.v1";
pub const REVISION_PIN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const REVISION_DRAIN_SCHEMA: &str = "kiana.revision-drain.v1";
pub const REVISION_DRAIN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_REVISION_TEXT: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRevisionPin {
    pub schema: String,
    pub version: SchemaVersion,
    pub revision_id: String,
    pub build_digest: String,
    pub workflow_digest: String,
    pub provider_digest: String,
    pub extension_catalog_digest: String,
    pub skill_trust_revision: String,
    pub replay_schema: String,
    pub pin_digest: String,
}

impl ExecutionRevisionPin {
    pub fn new(
        revision_id: impl Into<String>,
        build_digest: impl Into<String>,
        workflow_digest: impl Into<String>,
        provider_digest: impl Into<String>,
        extension_catalog_digest: impl Into<String>,
        skill_trust_revision: impl Into<String>,
        replay_schema: impl Into<String>,
    ) -> Result<Self, String> {
        let mut pin = Self {
            schema: REVISION_PIN_SCHEMA.to_owned(),
            version: REVISION_PIN_VERSION,
            revision_id: revision_id.into(),
            build_digest: build_digest.into(),
            workflow_digest: workflow_digest.into(),
            provider_digest: provider_digest.into(),
            extension_catalog_digest: extension_catalog_digest.into(),
            skill_trust_revision: skill_trust_revision.into(),
            replay_schema: replay_schema.into(),
            pin_digest: String::new(),
        };
        pin.pin_digest = pin.digest();
        pin.validate()?;
        Ok(pin)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REVISION_PIN_SCHEMA
            || self.version != REVISION_PIN_VERSION
            || !bounded(&self.revision_id)
            || !bounded(&self.replay_schema)
        {
            return Err("revision_pin_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.build_digest, "revision_build_digest"),
            (&self.workflow_digest, "revision_workflow_digest"),
            (&self.provider_digest, "revision_provider_digest"),
            (
                &self.extension_catalog_digest,
                "revision_extension_catalog_digest",
            ),
            (&self.skill_trust_revision, "revision_skill_trust_revision"),
            (&self.pin_digest, "revision_pin_digest"),
        ] {
            validate_digest(value, field)?;
        }
        if self.pin_digest != self.digest() {
            return Err("revision_pin_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn compatible_replay_with(&self, observed: &Self) -> Result<(), String> {
        self.validate()?;
        observed.validate()?;
        if self.replay_schema != observed.replay_schema {
            return Err("revision_replay_schema_unknown".to_owned());
        }
        if self.workflow_digest != observed.workflow_digest {
            return Err("revision_workflow_digest_drift".to_owned());
        }
        if self.provider_digest != observed.provider_digest {
            return Err("revision_provider_digest_drift".to_owned());
        }
        if self.extension_catalog_digest != observed.extension_catalog_digest {
            return Err("revision_extension_catalog_drift".to_owned());
        }
        if self.skill_trust_revision != observed.skill_trust_revision {
            return Err("revision_skill_trust_drift".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "revision_id": self.revision_id,
            "build_digest": self.build_digest,
            "workflow_digest": self.workflow_digest,
            "provider_digest": self.provider_digest,
            "extension_catalog_digest": self.extension_catalog_digest,
            "skill_trust_revision": self.skill_trust_revision,
            "replay_schema": self.replay_schema,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionDrainStatus {
    Active,
    Draining,
    Retired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionDrain {
    pub schema: String,
    pub version: SchemaVersion,
    pub pin: ExecutionRevisionPin,
    pub status: RevisionDrainStatus,
    pub drain_started_at_unix_ms: Option<u64>,
    pub drain_deadline_unix_ms: Option<u64>,
    pub active_run_count: u32,
    pub active_writer_count: u32,
    pub replacement_ready: bool,
    pub drain_digest: String,
}

impl RevisionDrain {
    pub fn new(pin: ExecutionRevisionPin) -> Result<Self, String> {
        pin.validate()?;
        let mut drain = Self {
            schema: REVISION_DRAIN_SCHEMA.to_owned(),
            version: REVISION_DRAIN_VERSION,
            pin,
            status: RevisionDrainStatus::Active,
            drain_started_at_unix_ms: None,
            drain_deadline_unix_ms: None,
            active_run_count: 0,
            active_writer_count: 0,
            replacement_ready: false,
            drain_digest: String::new(),
        };
        drain.drain_digest = drain.digest();
        drain.validate()?;
        Ok(drain)
    }

    pub fn begin(
        &mut self,
        now_unix_ms: u64,
        deadline_unix_ms: u64,
        replacement_ready: bool,
    ) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms == 0
            || deadline_unix_ms <= now_unix_ms
            || self.status != RevisionDrainStatus::Active
        {
            return Err("revision_drain_begin_invalid".to_owned());
        }
        self.status = RevisionDrainStatus::Draining;
        self.drain_started_at_unix_ms = Some(now_unix_ms);
        self.drain_deadline_unix_ms = Some(deadline_unix_ms);
        self.replacement_ready = replacement_ready;
        self.drain_digest = self.digest();
        self.validate()
    }

    pub fn observe(
        &mut self,
        active_run_count: u32,
        active_writer_count: u32,
    ) -> Result<(), String> {
        self.validate()?;
        if self.status != RevisionDrainStatus::Draining {
            return Err("revision_drain_not_active".to_owned());
        }
        self.active_run_count = active_run_count;
        self.active_writer_count = active_writer_count;
        self.drain_digest = self.digest();
        self.validate()
    }

    pub fn retire(&mut self, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        if self.status != RevisionDrainStatus::Draining
            || !self.replacement_ready
            || self.active_run_count > 0
            || self.active_writer_count > 0
        {
            return Err("revision_drain_not_empty".to_owned());
        }
        if self
            .drain_deadline_unix_ms
            .is_some_and(|deadline| now_unix_ms > deadline)
        {
            return Err("revision_drain_deadline_exceeded".to_owned());
        }
        self.status = RevisionDrainStatus::Retired;
        self.drain_digest = self.digest();
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REVISION_DRAIN_SCHEMA
            || self.version != REVISION_DRAIN_VERSION
            || self.drain_started_at_unix_ms == Some(0)
            || self.drain_deadline_unix_ms == Some(0)
            || (self.status == RevisionDrainStatus::Active
                && (self.drain_started_at_unix_ms.is_some()
                    || self.drain_deadline_unix_ms.is_some()))
        {
            return Err("revision_drain_invalid".to_owned());
        }
        self.pin.validate()?;
        validate_digest(&self.drain_digest, "revision_drain_digest")?;
        if self.drain_digest != self.digest() {
            return Err("revision_drain_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "pin": self.pin,
            "status": self.status,
            "drain_started_at_unix_ms": self.drain_started_at_unix_ms,
            "drain_deadline_unix_ms": self.drain_deadline_unix_ms,
            "active_run_count": self.active_run_count,
            "active_writer_count": self.active_writer_count,
            "replacement_ready": self.replacement_ready,
        }))
    }
}

fn bounded(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_REVISION_TEXT && !value.contains('\0')
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
