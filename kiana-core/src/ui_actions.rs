//! ControlPlane action facade for UI command CAS and durable reconciliation.
//!
//! Surface adapters may validate a cursor before making a request, but this module is the only
//! place that persists the action admission.  Each transition goes through the EventStore atomic
//! journal.  An uncertain commit is never converted to Applied: callers must query the original
//! command ID/idempotency key and use the stored record.

use super::*;
use kiana_domain::{
    derived_request_id, json_digest, AggregateVersion, CommitOutcome, RuntimeEvent,
    UiActionCommand, UiActionRecord, UiActionState, UiActionJournal,
    UI_ACTION_AGGREGATE_TYPE,
};
use serde_json::json;

const UI_ACTION_ACCEPTED_KIND: &str = "ui.action.accepted";
const UI_ACTION_APPLIED_KIND: &str = "ui.action.applied";
const UI_ACTION_REJECTED_KIND: &str = "ui.action.rejected";
const UI_ACTION_UNKNOWN_KIND: &str = "ui.action.unknown";

fn ui_action_error(reason: impl Into<String>) -> CoreError {
    CoreError::Port(PortError::Failed(reason.into()))
}

fn journal_digest(value: &str) -> String {
    value.strip_prefix("sha256:").unwrap_or(value).to_owned()
}

fn record_from_event(event: &RuntimeEvent) -> Result<UiActionRecord, CoreError> {
    let value = event
        .data
        .get("record")
        .cloned()
        .ok_or_else(|| ui_action_error("ui_action_record_missing"))?;
    let record: UiActionRecord = serde_json::from_value(value)
        .map_err(|_| ui_action_error("ui_action_record_decode_failed"))?;
    record
        .validate_for_query()
        .map_err(|error| ui_action_error(format!("ui_action_record_invalid:{error}")))?;
    Ok(record)
}

async fn read_ui_action_stream(
    events: &dyn EventStorePort,
    idempotency_key: &str,
) -> Result<Vec<RuntimeEvent>, CoreError> {
    events
        .read_stream(UI_ACTION_AGGREGATE_TYPE, idempotency_key)
        .await
        .map_err(CoreError::Port)
}

fn latest_record(events: &[RuntimeEvent]) -> Result<Option<UiActionRecord>, CoreError> {
    events
        .iter()
        .filter(|event| event.kind.starts_with("ui.action."))
        .map(record_from_event)
        .last()
        .transpose()
}

fn action_event(
    action: &UiActionCommand,
    record: &UiActionRecord,
    kind: &str,
    stream_version: u64,
    command_id: RequestId,
    idempotency_suffix: &str,
) -> Result<RuntimeEvent, CoreError> {
    let event = RuntimeEvent::new(
        command_id,
        stream_version,
        kind,
        json!({
            "schema": "kiana.ui-action-event.v1",
            "action": action,
            "record": record,
            "state": record.state,
            "effect_count": record.effect_count,
        }),
    )
    .map_err(|error| ui_action_error(format!("ui_action_event_invalid:{error}")))?
    .with_stream_metadata(
        UI_ACTION_AGGREGATE_TYPE,
        action.idempotency_key.clone(),
        stream_version,
    )
    .with_idempotency_key(format!(
        "ui-action:{}:{idempotency_suffix}",
        action.idempotency_key
    ));
    Ok(event)
}

async fn commit_ui_action_event(
    events: &dyn EventStorePort,
    action: &UiActionCommand,
    record: &UiActionRecord,
    kind: &str,
    stream_version: u64,
    command_id: RequestId,
    idempotency_suffix: &str,
) -> Result<CommitOutcome, CoreError> {
    if !events.supports_atomic_transitions() {
        return Err(ui_action_error("control_journal_required"));
    }
    let event = action_event(
        action,
        record,
        kind,
        stream_version,
        command_id,
        idempotency_suffix,
    )?;
    let batch = kiana_domain::TransitionBatch {
        command_id,
        command_digest: journal_digest(&json_digest(&json!({
            "action": action,
            "record": record,
            "kind": kind,
        }))),
        expected_versions: vec![AggregateVersion::new(
            UI_ACTION_AGGREGATE_TYPE,
            action.idempotency_key.clone(),
            stream_version.saturating_sub(1),
        )],
        events: vec![event],
    };
    events
        .commit_transition(batch)
        .await
        .map_err(CoreError::Port)
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionAuthoritySnapshot {
    pub authority_epoch: String,
    pub cursor: u64,
    #[serde(default)]
    pub target_revision: Option<u64>,
    pub owner_id: String,
    pub scope_digest: String,
    #[serde(default)]
    pub permit_digest: Option<String>,
    #[serde(default)]
    pub cancelled: bool,
}

impl UiActionAuthoritySnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.authority_epoch.trim().is_empty()
            || self.authority_epoch.len() > 256
            || self.target_revision == Some(0)
            || self.owner_id.trim().is_empty()
            || self.owner_id.len() > 256
            || !self.scope_digest.starts_with("sha256:")
            || self.scope_digest.len() != 71
        {
            return Err("ui_action_authority_snapshot_invalid".to_owned());
        }
        if self
            .permit_digest
            .as_deref()
            .is_some_and(|digest| !digest.starts_with("sha256:") || digest.len() != 71)
        {
            return Err("ui_action_permit_digest_invalid".to_owned());
        }
        Ok(())
    }
}

impl ControlPlane {
    /// Persist one UI action admission with CAS and idempotency semantics.
    pub async fn admit_ui_action(
        &self,
        action: &UiActionCommand,
        authority: &UiActionAuthoritySnapshot,
        now_unix_ms: u64,
    ) -> Result<UiActionRecord, CoreError> {
        action
            .validate()
            .map_err(|error| ui_action_error(format!("ui_action_invalid:{error}")))?;
        authority
            .validate()
            .map_err(|error| ui_action_error(format!("ui_action_authority_invalid:{error}")))?;
        if authority.cancelled {
            return Err(ui_action_error("ui_action_cancelled"));
        }
        if authority.permit_digest.is_none() {
            return Err(ui_action_error("ui_action_permit_required"));
        }
        let events = read_ui_action_stream(self.events.as_ref(), &action.idempotency_key).await?;
        if let Some(original) = latest_record(&events)? {
            if original.command_digest != action.command_digest {
                return Err(ui_action_error("ui_action_idempotency_digest_mismatch"));
            }
            if original.owner_id != action.owner_id || original.owner_id != authority.owner_id {
                return Err(ui_action_error("ui_action_owner_mismatch"));
            }
            if original.scope_digest != action.scope_digest
                || original.scope_digest != authority.scope_digest
            {
                return Err(ui_action_error("ui_action_scope_mismatch"));
            }
            // A replay returns the original durable result.  It never dispatches a second effect.
            return Ok(original);
        }
        let mut journal = UiActionJournal::new();
        let (record, replayed) = journal
            .accept(
                action,
                &authority.authority_epoch,
                authority.cursor,
                authority.target_revision,
                &authority.owner_id,
                &authority.scope_digest,
                now_unix_ms,
            )
            .map_err(|error| ui_action_error(format!("ui_action_rejected:{error}")))?;
        debug_assert!(!replayed);
        let outcome = commit_ui_action_event(
            self.events.as_ref(),
            action,
            &record,
            UI_ACTION_ACCEPTED_KIND,
            1,
            action.command_id,
            "accepted",
        )
        .await?;
        match outcome {
            CommitOutcome::Committed { .. } => Ok(record),
            CommitOutcome::Replayed { .. } => self
                .query_original_ui_action(&action.idempotency_key)
                .await
                .map(|value| value.ok_or_else(|| ui_action_error("ui_action_record_missing")))?,
            CommitOutcome::Conflict { .. } => Err(ui_action_error("ui_action_cas_conflict")),
            CommitOutcome::Unknown { .. } => Err(ui_action_error(
                "result_unknown:ui_action_accept_unconfirmed",
            )),
        }
    }

    /// Mark a previously accepted action Applied exactly once after the effect receipt exists.
    pub async fn apply_ui_action(
        &self,
        action: &UiActionCommand,
        receipt_digest: &str,
        now_unix_ms: u64,
    ) -> Result<UiActionRecord, CoreError> {
        let events = read_ui_action_stream(self.events.as_ref(), &action.idempotency_key).await?;
        let current = latest_record(&events)?.ok_or_else(|| ui_action_error("ui_action_not_accepted"))?;
        if current.command_digest != action.command_digest {
            return Err(ui_action_error("ui_action_idempotency_digest_mismatch"));
        }
        let next = current
            .apply_transition(receipt_digest.to_owned(), now_unix_ms)
            .map_err(|error| ui_action_error(format!("ui_action_apply_rejected:{error}")))?;
        if next.state == UiActionState::Applied && current.state == UiActionState::Applied {
            return Ok(current);
        }
        let outcome = commit_ui_action_event(
            self.events.as_ref(),
            action,
            &next,
            UI_ACTION_APPLIED_KIND,
            events.len() as u64 + 1,
            derived_request_id("ui.action.apply", &action.command_id.to_string()),
            "applied",
        )
        .await?;
        match outcome {
            CommitOutcome::Committed { .. } => Ok(next),
            CommitOutcome::Replayed { .. } => self
                .query_original_ui_action(&action.idempotency_key)
                .await
                .map(|value| value.ok_or_else(|| ui_action_error("ui_action_record_missing")))?,
            CommitOutcome::Conflict { .. } => Err(ui_action_error("ui_action_cas_conflict")),
            CommitOutcome::Unknown { .. } => Err(ui_action_error(
                "result_unknown:ui_action_apply_unconfirmed",
            )),
        }
    }

    async fn finish_ui_action(
        &self,
        action: &UiActionCommand,
        reason: &str,
        unknown: bool,
    ) -> Result<UiActionRecord, CoreError> {
        let events = read_ui_action_stream(self.events.as_ref(), &action.idempotency_key).await?;
        let current = latest_record(&events)?.ok_or_else(|| ui_action_error("ui_action_not_accepted"))?;
        if current.command_digest != action.command_digest {
            return Err(ui_action_error("ui_action_idempotency_digest_mismatch"));
        }
        let next = if unknown {
            current.unknown_transition(reason.to_owned())
        } else {
            current.reject_transition(reason.to_owned())
        }
        .map_err(|error| ui_action_error(format!("ui_action_terminal_rejected:{error}")))?;
        if next.state == current.state {
            return Ok(current);
        }
        let kind = if unknown {
            UI_ACTION_UNKNOWN_KIND
        } else {
            UI_ACTION_REJECTED_KIND
        };
        let suffix = if unknown { "unknown" } else { "rejected" };
        let outcome = commit_ui_action_event(
            self.events.as_ref(),
            action,
            &next,
            kind,
            events.len() as u64 + 1,
            derived_request_id(&format!("ui.action.{suffix}"), &action.command_id.to_string()),
            suffix,
        )
        .await?;
        match outcome {
            CommitOutcome::Committed { .. } => Ok(next),
            CommitOutcome::Replayed { .. } => self
                .query_original_ui_action(&action.idempotency_key)
                .await
                .map(|value| value.ok_or_else(|| ui_action_error("ui_action_record_missing")))?,
            CommitOutcome::Conflict { .. } => Err(ui_action_error("ui_action_cas_conflict")),
            CommitOutcome::Unknown { .. } => Err(ui_action_error(
                "result_unknown:ui_action_terminal_unconfirmed",
            )),
        }
    }

    pub async fn reject_ui_action(
        &self,
        action: &UiActionCommand,
        reason: &str,
    ) -> Result<UiActionRecord, CoreError> {
        self.finish_ui_action(action, reason, false).await
    }

    pub async fn mark_ui_action_unknown(
        &self,
        action: &UiActionCommand,
        reason: &str,
    ) -> Result<UiActionRecord, CoreError> {
        self.finish_ui_action(action, reason, true).await
    }

    /// Reconcile a lost ACK using the original idempotency key; a new command ID is never used.
    pub async fn query_original_ui_action(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<UiActionRecord>, CoreError> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
            return Err(ui_action_error("ui_action_idempotency_key_invalid"));
        }
        let events = read_ui_action_stream(self.events.as_ref(), idempotency_key).await?;
        latest_record(&events)
    }
}
