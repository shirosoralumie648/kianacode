//! Local authentication/session adapter for the daemon boundary.
//!
//! This adapter only manages opaque `AuthenticatedPrincipalRef` plus short-lived session
//! assertions. It never stores bearer values, assigns roles, or calls the Broker. The in-memory
//! session map is intentionally a compatibility adapter until the durable identity steps land.

use kiana_domain::{
    AuthenticatedPrincipalRef, AuthenticationAssurance, SessionAssertion, SessionStatus,
};
use kiana_ports::PortError;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct LocalAuthnAdapter {
    principal: AuthenticatedPrincipalRef,
    sessions: Arc<RwLock<BTreeMap<String, SessionAssertion>>>,
}

impl LocalAuthnAdapter {
    pub fn new(principal: AuthenticatedPrincipalRef) -> Result<Self, PortError> {
        principal
            .validate()
            .map_err(|error| PortError::Failed(format!("AUTH_PRINCIPAL_INVALID:{error}")))?;
        Ok(Self {
            principal,
            sessions: Arc::new(RwLock::new(BTreeMap::new())),
        })
    }

    pub fn principal(&self) -> AuthenticatedPrincipalRef {
        self.principal.clone()
    }

    pub fn open_session(
        &self,
        session_id: impl Into<String>,
        now_unix_ms: u64,
        ttl_ms: u64,
        assurance: AuthenticationAssurance,
    ) -> Result<SessionAssertion, PortError> {
        if ttl_ms == 0 {
            return Err(PortError::Failed("AUTH_SESSION_TTL_INVALID".to_owned()));
        }
        let expires_at = now_unix_ms
            .checked_add(ttl_ms)
            .ok_or_else(|| PortError::Failed("AUTH_SESSION_TTL_INVALID".to_owned()))?;
        let assertion = SessionAssertion::new(
            session_id,
            self.principal.clone(),
            assurance,
            now_unix_ms,
            expires_at,
            self.principal.credential_generation,
            1,
        )
        .map_err(PortError::Failed)?;
        let key = assertion.session_id.as_str().to_owned();
        let mut sessions = self
            .sessions
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sessions.contains_key(&key) {
            return Err(PortError::Conflict("AUTH_SESSION_REPLAY".to_owned()));
        }
        sessions.insert(key, assertion.clone());
        Ok(assertion)
    }

    /// Validate a session if it is known by this compatibility adapter. Unknown legacy sessions
    /// remain unresolved unless the ingress explicitly requests protected-local authentication.
    pub fn validate_if_present(
        &self,
        session_id: &str,
        now_unix_ms: u64,
        protected: bool,
    ) -> Result<Option<SessionAssertion>, PortError> {
        if session_id.trim().is_empty() {
            return Err(PortError::Failed("AUTH_SESSION_MISSING".to_owned()));
        }
        let assertion = self
            .sessions
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .cloned();
        let Some(assertion) = assertion else {
            if protected {
                return Err(PortError::Failed("AUTH_SESSION_MISSING".to_owned()));
            }
            return Ok(None);
        };
        assertion
            .validate_at(now_unix_ms)
            .map_err(PortError::Failed)?;
        if assertion.principal != self.principal {
            return Err(PortError::Failed("AUTH_PRINCIPAL_MISMATCH".to_owned()));
        }
        Ok(Some(assertion))
    }

    pub fn authenticate(
        &self,
        session_id: &str,
        now_unix_ms: u64,
    ) -> Result<AuthenticatedPrincipalRef, PortError> {
        self.validate_if_present(session_id, now_unix_ms, true)?
            .map(|_| self.principal.clone())
            .ok_or_else(|| PortError::Failed("AUTH_SESSION_MISSING".to_owned()))
    }

    pub fn revoke_session(&self, session_id: &str) -> Result<SessionAssertion, PortError> {
        let mut sessions = self
            .sessions
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let assertion = sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| PortError::Failed("AUTH_SESSION_MISSING".to_owned()))?;
        let revoked = assertion.revoke().map_err(PortError::Failed)?;
        sessions.insert(session_id.to_owned(), revoked.clone());
        Ok(revoked)
    }

    pub fn session_snapshot(&self, session_id: &str) -> Option<SessionAssertion> {
        self.sessions
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .cloned()
    }

    pub(crate) fn now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_millis()).ok())
            .unwrap_or(0)
    }

    #[allow(dead_code)]
    pub(crate) fn status(session_id: &str, assertion: Option<&SessionAssertion>) -> SessionStatus {
        assertion
            .filter(|value| value.session_id.as_str() == session_id)
            .map_or(SessionStatus::Proposed, |value| value.status)
    }
}
