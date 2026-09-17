//! Monotonic authority/session/policy fence values.
//!
//! A fence is an immutable observation attached to a command or permit. It is not a capability;
//! the ControlPlane must issue and recheck it at the effect boundary. Parent digest and sequence
//! links make stale/late observations explicit instead of allowing a prior revision to resurrect.

use crate::{json_digest, FenceTokenId, SchemaVersion, SecurityReasonCode, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const AUTHORITY_FENCE_SCHEMA: &str = "kiana.authority-fence.v1";
pub const AUTHORITY_FENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityFence {
    pub schema: String,
    pub version: SchemaVersion,
    pub fence_id: FenceTokenId,
    pub scope: String,
    pub session_id: SessionId,
    pub authority_epoch: u64,
    pub session_generation: u64,
    pub policy_revision: String,
    pub config_revision: String,
    pub sequence: u64,
    #[serde(default)]
    pub parent_digest: Option<String>,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub fence_digest: String,
}

impl AuthorityFence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fence_id: FenceTokenId,
        scope: impl Into<String>,
        session_id: impl Into<String>,
        authority_epoch: u64,
        session_generation: u64,
        policy_revision: impl Into<String>,
        config_revision: impl Into<String>,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        Self::build(
            fence_id,
            scope.into(),
            SessionId::new(session_id),
            authority_epoch,
            session_generation,
            policy_revision.into(),
            config_revision.into(),
            1,
            None,
            issued_at_unix_ms,
            expires_at_unix_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn successor(
        previous: &Self,
        authority_epoch: u64,
        session_generation: u64,
        policy_revision: impl Into<String>,
        config_revision: impl Into<String>,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        previous.validate()?;
        if authority_epoch < previous.authority_epoch {
            return Err(SecurityReasonCode::PolicyAuthorityEpochRollback
                .as_str()
                .to_owned());
        }
        if session_generation < previous.session_generation {
            return Err(SecurityReasonCode::AuthSessionGenerationStale
                .as_str()
                .to_owned());
        }
        let sequence = previous
            .sequence
            .checked_add(1)
            .ok_or_else(|| "authority_fence_sequence_exhausted".to_owned())?;
        Self::build(
            previous.fence_id,
            previous.scope.clone(),
            previous.session_id.clone(),
            authority_epoch,
            session_generation,
            policy_revision.into(),
            config_revision.into(),
            sequence,
            Some(previous.fence_digest.clone()),
            issued_at_unix_ms,
            expires_at_unix_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        fence_id: FenceTokenId,
        scope: String,
        session_id: SessionId,
        authority_epoch: u64,
        session_generation: u64,
        policy_revision: String,
        config_revision: String,
        sequence: u64,
        parent_digest: Option<String>,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut fence = Self {
            schema: AUTHORITY_FENCE_SCHEMA.to_owned(),
            version: AUTHORITY_FENCE_VERSION,
            fence_id,
            scope,
            session_id,
            authority_epoch,
            session_generation,
            policy_revision,
            config_revision,
            sequence,
            parent_digest,
            issued_at_unix_ms,
            expires_at_unix_ms,
            fence_digest: String::new(),
        };
        fence.fence_digest = fence.digest();
        fence.validate()?;
        Ok(fence)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let fence: Self = serde_json::from_value(value.clone())
            .map_err(|_| "authority_fence_decode_failed".to_owned())?;
        fence.validate()?;
        Ok(fence)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "authority_fence_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHORITY_FENCE_SCHEMA
            || !self.version.is_compatible_with(&AUTHORITY_FENCE_VERSION)
            || self.fence_id.as_uuid().is_nil()
            || self.scope.trim().is_empty()
            || self.scope.len() > 512
            || self.scope.contains('\0')
            || self.session_id.is_empty()
            || self.authority_epoch == 0
            || self.session_generation == 0
            || self.sequence == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
        {
            return Err("authority_fence_header_invalid".to_owned());
        }
        validate_digest(&self.policy_revision, "authority_fence_policy_revision")?;
        validate_digest(&self.config_revision, "authority_fence_config_revision")?;
        if self.sequence == 1 {
            if self.parent_digest.is_some() {
                return Err("authority_fence_genesis_parent_unexpected".to_owned());
            }
        } else if self.parent_digest.is_none() {
            return Err("authority_fence_parent_required".to_owned());
        } else {
            validate_digest(
                self.parent_digest.as_deref().unwrap_or_default(),
                "authority_fence_parent_digest",
            )?;
        }
        validate_digest(&self.fence_digest, "authority_fence_digest")?;
        if self.fence_digest != self.digest() {
            return Err("authority_fence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_successor(&self, previous: &Self) -> Result<(), String> {
        previous.validate()?;
        self.validate()?;
        if self.fence_id != previous.fence_id || self.scope != previous.scope {
            return Err(SecurityReasonCode::FactFenceMismatch.as_str().to_owned());
        }
        if self.sequence <= previous.sequence {
            return Err("authority_fence_sequence_rollback".to_owned());
        }
        if self.authority_epoch < previous.authority_epoch {
            return Err(SecurityReasonCode::PolicyAuthorityEpochRollback
                .as_str()
                .to_owned());
        }
        if self.session_generation < previous.session_generation {
            return Err(SecurityReasonCode::AuthSessionGenerationStale
                .as_str()
                .to_owned());
        }
        if self.parent_digest.as_deref() != Some(previous.fence_digest.as_str()) {
            return Err(SecurityReasonCode::FactFenceMismatch.as_str().to_owned());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn validate_current(
        &self,
        now_unix_ms: u64,
        authority_epoch: u64,
        session_generation: u64,
        policy_revision: &str,
        config_revision: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms < self.issued_at_unix_ms || now_unix_ms >= self.expires_at_unix_ms {
            return Err(SecurityReasonCode::UnknownFenceExpired.as_str().to_owned());
        }
        if authority_epoch < self.authority_epoch {
            return Err(SecurityReasonCode::PolicyAuthorityEpochRollback
                .as_str()
                .to_owned());
        }
        if authority_epoch != self.authority_epoch {
            return Err(SecurityReasonCode::PolicyAuthorityEpochStale
                .as_str()
                .to_owned());
        }
        if session_generation != self.session_generation {
            return Err(SecurityReasonCode::AuthSessionGenerationStale
                .as_str()
                .to_owned());
        }
        if policy_revision != self.policy_revision {
            return Err(SecurityReasonCode::PolicyRevisionStale.as_str().to_owned());
        }
        if config_revision != self.config_revision {
            return Err(SecurityReasonCode::PolicyConfigRevisionStale
                .as_str()
                .to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "fence_id": self.fence_id,
            "scope": self.scope,
            "session_id": self.session_id,
            "authority_epoch": self.authority_epoch,
            "session_generation": self.session_generation,
            "policy_revision": self.policy_revision,
            "config_revision": self.config_revision,
            "sequence": self.sequence,
            "parent_digest": self.parent_digest,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
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
