//! Pure Web tab/session state contracts used by presenters.
//!
//! These values deliberately stay client-local.  A tab may keep a draft and a stable submission
//! key, but it cannot manufacture the authenticated principal, owner lease, permission or feed
//! cursor that the server returns and rechecks.

use kiana_domain::{RequestId, SessionId};
use kiana_protocol::{UiActionSubmissionV1, UiTabSessionV1};
use serde::{Deserialize, Serialize};

pub const WEB_CLIENT_TAB_SCHEMA: &str = "kiana.web-client-tab.v1";
pub const WEB_CLIENT_DRAFT_SCHEMA: &str = "kiana.web-client-draft.v1";
pub const WEB_CLIENT_SUBMISSION_SCHEMA: &str = "kiana.web-client-submission.v1";
pub const WEB_CLIENT_MAX_DRAFT_BYTES: usize = 64 * 1024;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

/// One tab's local scope.  `server_lease` is optional before bootstrap and is never accepted as
/// authority for a mutation; it is only the last server projection seen by this client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebClientTab {
    pub schema: String,
    pub tab_id: String,
    pub session_id: SessionId,
    #[serde(default)]
    pub server_lease: Option<UiTabSessionV1>,
}

impl WebClientTab {
    pub fn new(tab_id: impl Into<String>, session_id: impl Into<String>) -> Result<Self, String> {
        let tab = Self {
            schema: WEB_CLIENT_TAB_SCHEMA.to_owned(),
            tab_id: tab_id.into(),
            session_id: SessionId::new(session_id),
            server_lease: None,
        };
        tab.validate()?;
        Ok(tab)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_CLIENT_TAB_SCHEMA {
            return Err("web_client_tab_schema_invalid".to_owned());
        }
        required(&self.tab_id, "web_client_tab_id", 256)?;
        required(self.session_id.as_str(), "web_client_session_id", 256)?;
        if let Some(lease) = &self.server_lease {
            lease.validate()?;
            if lease.tab_id != self.tab_id || lease.session_id != self.session_id {
                return Err("web_client_tab_lease_scope_mismatch".to_owned());
            }
        }
        Ok(())
    }

    pub fn can_mutate(&self) -> bool {
        self.server_lease.as_ref().is_some_and(|lease| {
            lease.disposition == kiana_protocol::UiTabSessionDisposition::Owner && !lease.feed_only
        })
    }
}

/// Draft text is keyed by this tab/session pair and is never sent as an implicit command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebClientDraft {
    pub schema: String,
    pub tab_id: String,
    pub session_id: SessionId,
    pub text: String,
    pub revision: u64,
}

impl WebClientDraft {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_CLIENT_DRAFT_SCHEMA || self.revision == 0 {
            return Err("web_client_draft_header_invalid".to_owned());
        }
        required(&self.tab_id, "web_client_draft_tab", 256)?;
        required(self.session_id.as_str(), "web_client_draft_session", 256)?;
        if self.text.len() > WEB_CLIENT_MAX_DRAFT_BYTES {
            return Err("web_client_draft_too_large".to_owned());
        }
        Ok(())
    }
}

/// Stable submission identity used for double-click/reconnect reconciliation.  The client must
/// reuse it for a retry and display the original server result; it must not synthesize a second
/// command ID after an unknown response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebClientSubmission {
    pub schema: String,
    pub command_id: RequestId,
    pub idempotency_key: String,
    pub tab_id: String,
    pub session_id: SessionId,
    pub expected_revision: Option<u64>,
    pub submitted_at_unix_ms: u64,
}

impl WebClientSubmission {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_CLIENT_SUBMISSION_SCHEMA
            || self.command_id.as_uuid().is_nil()
            || self.expected_revision == Some(0)
            || self.submitted_at_unix_ms == 0
        {
            return Err("web_client_submission_header_invalid".to_owned());
        }
        required(
            &self.idempotency_key,
            "web_client_submission_idempotency",
            256,
        )?;
        required(&self.tab_id, "web_client_submission_tab", 256)?;
        required(
            self.session_id.as_str(),
            "web_client_submission_session",
            256,
        )
    }

    pub fn protocol(
        &self,
        expected_epoch: impl Into<String>,
        expected_cursor: u64,
        payload_digest: impl Into<String>,
    ) -> UiActionSubmissionV1 {
        UiActionSubmissionV1 {
            schema: kiana_protocol::UI_ACTION_SUBMISSION_SCHEMA.to_owned(),
            command_id: self.command_id,
            idempotency_key: self.idempotency_key.clone(),
            session_id: self.session_id.clone(),
            tab_id: self.tab_id.clone(),
            expected_epoch: expected_epoch.into(),
            expected_cursor,
            expected_revision: self.expected_revision,
            payload_digest: payload_digest.into(),
            submitted_at_unix_ms: self.submitted_at_unix_ms,
        }
    }
}
