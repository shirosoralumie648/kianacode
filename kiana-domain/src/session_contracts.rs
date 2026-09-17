//! Versioned session assertions used by authentication adapters.
//!
//! A session assertion is a bounded, revocable snapshot. It carries no bearer value and does not
//! grant a role or capability; callers still need the ControlPlane context/policy path.

use crate::{json_digest, AuthenticatedPrincipalRef, SchemaVersion, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SESSION_ASSERTION_SCHEMA: &str = "kiana.session-assertion.v1";
pub const SESSION_ASSERTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationAssurance {
    Anonymous,
    Local,
    ProtectedLocal,
    Strong,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Proposed,
    Active,
    Suspended,
    Revoked,
    Expired,
}

impl SessionStatus {
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Revoked | Self::Expired)
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Proposed, Self::Active | Self::Revoked)
                | (
                    Self::Active,
                    Self::Suspended | Self::Revoked | Self::Expired
                )
                | (Self::Suspended, Self::Revoked | Self::Expired)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAssertion {
    pub schema: String,
    pub version: SchemaVersion,
    pub session_id: SessionId,
    pub principal: AuthenticatedPrincipalRef,
    pub assurance: AuthenticationAssurance,
    pub status: SessionStatus,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub credential_generation: u64,
    pub authority_epoch: u64,
    pub session_digest: String,
}

impl SessionAssertion {
    pub fn new(
        session_id: impl Into<String>,
        principal: AuthenticatedPrincipalRef,
        assurance: AuthenticationAssurance,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
        credential_generation: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut assertion = Self {
            schema: SESSION_ASSERTION_SCHEMA.to_owned(),
            version: SESSION_ASSERTION_VERSION,
            session_id: SessionId::new(session_id),
            principal,
            assurance,
            status: SessionStatus::Active,
            issued_at_unix_ms,
            expires_at_unix_ms,
            credential_generation,
            authority_epoch,
            session_digest: String::new(),
        };
        assertion.session_digest = assertion.digest();
        assertion.validate()?;
        Ok(assertion)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let assertion: Self = serde_json::from_value(value.clone())
            .map_err(|_| "session_assertion_decode_failed".to_owned())?;
        assertion.validate()?;
        Ok(assertion)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "session_assertion_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SESSION_ASSERTION_SCHEMA
            || !self.version.is_compatible_with(&SESSION_ASSERTION_VERSION)
            || self.session_id.is_empty()
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.credential_generation == 0
            || self.authority_epoch == 0
        {
            return Err("session_assertion_header_invalid".to_owned());
        }
        self.principal.validate()?;
        if self.assurance == AuthenticationAssurance::Anonymous
            && self.status == SessionStatus::Active
        {
            return Err("session_assertion_anonymous_active".to_owned());
        }
        validate_digest(&self.session_digest, "session_assertion_digest")?;
        if self.session_digest != self.digest() {
            return Err("session_assertion_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn active_at(&self, now_unix_ms: u64) -> bool {
        self.status == SessionStatus::Active
            && now_unix_ms >= self.issued_at_unix_ms
            && now_unix_ms < self.expires_at_unix_ms
    }

    pub fn transition(&self, next: SessionStatus) -> Result<Self, String> {
        if !self.status.can_transition_to(next) {
            return Err("session_assertion_transition_invalid".to_owned());
        }
        let mut next_assertion = self.clone();
        next_assertion.status = next;
        next_assertion.session_digest = next_assertion.digest();
        next_assertion.validate()?;
        Ok(next_assertion)
    }

    pub fn revoke(&self) -> Result<Self, String> {
        self.transition(SessionStatus::Revoked)
    }

    pub fn validate_at(&self, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        if self.status == SessionStatus::Revoked {
            return Err("AUTH_SESSION_REVOKED".to_owned());
        }
        if now_unix_ms >= self.expires_at_unix_ms || self.status == SessionStatus::Expired {
            return Err("AUTH_SESSION_EXPIRED".to_owned());
        }
        if !self.active_at(now_unix_ms) {
            return Err("AUTH_SESSION_INACTIVE".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "session_id": self.session_id,
            "principal": self.principal,
            "assurance": self.assurance,
            "status": self.status,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "credential_generation": self.credential_generation,
            "authority_epoch": self.authority_epoch,
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
