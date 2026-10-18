//! Typed Web Human Inbox and action-card presenter contracts.
//!
//! This module keeps only a bounded server projection and emits a versioned intent.  It never
//! chooses an actor, creates an approval, expands a scope or executes a command.  The Web server
//! and ControlPlane must recheck the owner lease, inbox/action revision, expiry, payload digest
//! and idempotency key when the intent crosses the transport boundary.

use crate::web_contract::WebClientTab;
use kiana_protocol::{
    json_digest, UiHumanActionCardV1, UiHumanActionFieldV1, UiHumanActionIntentV1, UiHumanInboxV1,
    UI_HUMAN_ACTION_INTENT_SCHEMA,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub const WEB_HUMAN_INBOX_SCHEMA: &str = "kiana.web-human-inbox.v1";
pub const WEB_HUMAN_MAX_CARDS: usize = 256;
pub const WEB_HUMAN_MAX_FIELD_BYTES: usize = 64 * 1024;
pub const WEB_HUMAN_MAX_FIELD_STRING_BYTES: usize = 4 * 1024;
pub const WEB_HUMAN_MAX_FIELD_LIST_ITEMS: usize = 64;

/// Declared display field types.  An unknown type fails closed instead of being passed through.
pub const WEB_HUMAN_FIELD_TYPES: [&str; 4] = ["json", "string", "boolean", "string_list"];

/// Resolve decisions the current backend cannot honor.  `human.resolve` dispatch only reaches
/// approval approve/deny, company/company.business, failure reconcile/recovery and feedback
/// review; there is no approval amend, workspace restore or run continue inside `human.resolve`.
/// The presenter therefore refuses these ids with [`WEB_HUMAN_REFUSAL_ACTION_UNSUPPORTED`] rather
/// than emitting an intent that would be classified as `unmapped` and silently do nothing.
pub const WEB_HUMAN_UNSUPPORTED_ACTIONS: [&str; 3] = ["edit", "restore", "continue"];
/// Stable refusal code prefix; the action id is appended.
pub const WEB_HUMAN_REFUSAL_ACTION_UNSUPPORTED: &str = "web_human_resolve_action_unsupported";

/// Deny-first display state of one card.  A revoked card stays revoked even after a later
/// non-revoked projection would refresh it; expiry is derived from the server card only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebHumanActionState {
    Actionable,
    Expired,
    Revoked,
}

impl WebHumanActionState {
    pub const ALL: [Self; 3] = [Self::Actionable, Self::Expired, Self::Revoked];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Actionable => "actionable",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }

    pub fn is_actionable(self) -> bool {
        self == Self::Actionable
    }
}

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

fn validate_field_text(text: &str) -> Result<(), String> {
    if text.len() > WEB_HUMAN_MAX_FIELD_STRING_BYTES {
        return Err("web_human_field_value_too_long".to_owned());
    }
    if text.contains('\0') {
        return Err("web_human_field_value_type_invalid".to_owned());
    }
    Ok(())
}

/// Validate one submitted field value against the server card descriptor.  `null` explicitly
/// clears an *optional* field; a required field cannot be cleared, so a `null` required value is
/// the same refusal as omitting it.  Everything else must match the declared type, size and
/// allowed values, so an illegal value never produces an intent.
fn validate_field_value(field: &UiHumanActionFieldV1, value: &Value) -> Result<(), String> {
    if value.is_null() {
        if field.required {
            return Err("web_human_required_field_missing".to_owned());
        }
        return Ok(());
    }
    match field.field_type.as_str() {
        "json" => Ok(()),
        "string" => {
            let text = value
                .as_str()
                .ok_or_else(|| "web_human_field_value_type_invalid".to_owned())?;
            validate_field_text(text)?;
            if field.required && text.trim().is_empty() {
                return Err("web_human_field_value_empty".to_owned());
            }
            if !field.allowed_values.is_empty()
                && !field.allowed_values.iter().any(|allowed| allowed == text)
            {
                return Err("web_human_field_value_not_allowed".to_owned());
            }
            Ok(())
        }
        "boolean" => value
            .as_bool()
            .map(|_| ())
            .ok_or_else(|| "web_human_field_value_type_invalid".to_owned()),
        "string_list" => {
            let items = value
                .as_array()
                .ok_or_else(|| "web_human_field_value_type_invalid".to_owned())?;
            if items.len() > WEB_HUMAN_MAX_FIELD_LIST_ITEMS {
                return Err("web_human_field_value_limit".to_owned());
            }
            for item in items {
                let text = item
                    .as_str()
                    .ok_or_else(|| "web_human_field_value_type_invalid".to_owned())?;
                validate_field_text(text)?;
            }
            Ok(())
        }
        _ => Err("web_human_field_type_unknown".to_owned()),
    }
}

fn card_key(item_id: &str, action_id: &str) -> String {
    // IDs are opaque and may contain `:`. Length-delimit the first component so distinct
    // `(item_id, action_id)` tuples cannot collapse into one BTreeMap entry.
    format!("{}:{}{}", item_id.len(), item_id, action_id)
}

/// Bounded client-side projection keyed by the server's item/action identity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebHumanInbox {
    schema: String,
    revision: String,
    cards: BTreeMap<String, UiHumanActionCardV1>,
    limitations: Vec<String>,
}

impl WebHumanInbox {
    /// Replace the projection at one server revision.  Duplicate cards fail before any button or
    /// intent can be created; stale revision handling remains a server CAS decision.
    pub fn replace(&mut self, projection: UiHumanInboxV1) -> Result<(), String> {
        projection.validate()?;
        if projection.items.len() > WEB_HUMAN_MAX_CARDS {
            return Err("web_human_inbox_cards_limit".to_owned());
        }
        let mut cards = BTreeMap::new();
        for card in projection.items {
            let key = card_key(&card.item_id, &card.action_id);
            if cards.insert(key, card).is_some() {
                return Err("web_human_inbox_duplicate_card".to_owned());
            }
        }
        self.schema = WEB_HUMAN_INBOX_SCHEMA.to_owned();
        self.revision = projection.revision;
        self.cards = cards;
        self.limitations = projection.limitations;
        Ok(())
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }

    pub fn cards(&self) -> impl Iterator<Item = &UiHumanActionCardV1> {
        self.cards.values()
    }

    pub fn get(&self, item_id: &str, action_id: &str) -> Option<&UiHumanActionCardV1> {
        self.cards.get(&card_key(item_id, action_id))
    }

    /// Single deny-first state derivation for a card: revoked outranks expired, and neither can
    /// ever become actionable by a later client-side refresh.
    pub fn action_state(
        &self,
        item_id: &str,
        action_id: &str,
        now_unix_ms: u64,
    ) -> Option<WebHumanActionState> {
        let card = self.get(item_id, action_id)?;
        if card.revoked {
            return Some(WebHumanActionState::Revoked);
        }
        if card.is_expired(now_unix_ms) {
            return Some(WebHumanActionState::Expired);
        }
        Some(WebHumanActionState::Actionable)
    }

    /// Cards a presenter may render as actionable at this instant.  Expired and revoked cards stay
    /// visible in the projection but are never offered for submission.
    pub fn actionable(&self, now_unix_ms: u64) -> Vec<&UiHumanActionCardV1> {
        self.cards
            .values()
            .filter(|card| !card.revoked && !card.is_expired(now_unix_ms))
            .collect()
    }

    /// Stable submission identity for a card decision.  Repeated clicks and a lost response must
    /// reuse this key so the server replays the original command result instead of creating a
    /// second effect.  The key is derived from the server card identity and the submitted fields;
    /// it carries no actor, scope or approval material.
    pub fn submission_key(
        &self,
        item_id: &str,
        action_id: &str,
        fields: &Value,
    ) -> Result<String, String> {
        required(item_id, "web_human_item_id", 256)?;
        required(action_id, "web_human_action_id", 256)?;
        let card = self
            .get(item_id, action_id)
            .ok_or_else(|| "web_human_action_not_found".to_owned())?;
        Ok(format!(
            "web-human:{}",
            json_digest(&serde_json::json!({
                "schema": WEB_HUMAN_INBOX_SCHEMA,
                "item_id": item_id,
                "action_id": action_id,
                "inbox_revision": self.revision,
                "expected_revision": card.expected_revision,
                "payload_digest": card.payload_digest,
                "fields": fields,
            }))
        ))
    }

    /// Validate a form and produce an intent only.  The returned object deliberately omits actor,
    /// scope and approval material; the authoritative server resolves those from the card.
    pub fn prepare_intent(
        &self,
        tab: &WebClientTab,
        item_id: &str,
        action_id: &str,
        fields: Value,
        idempotency_key: &str,
        observed_inbox_revision: &str,
        observed_card_revision: Option<u64>,
        now_unix_ms: u64,
    ) -> Result<UiHumanActionIntentV1, String> {
        tab.validate()?;
        if !tab.can_mutate() {
            return Err("web_human_inbox_owner_required".to_owned());
        }
        required(item_id, "web_human_item_id", 256)?;
        required(action_id, "web_human_action_id", 256)?;
        required(idempotency_key, "web_human_idempotency", 256)?;
        if observed_inbox_revision != self.revision {
            return Err("web_human_inbox_stale_revision".to_owned());
        }
        let card = self
            .get(item_id, action_id)
            .ok_or_else(|| "web_human_action_not_found".to_owned())?;
        card.validate()?;
        // The backend cannot honor these decisions through `human.resolve`; refuse before any
        // form value is interpreted so the presenter never emits a doomed intent.
        if WEB_HUMAN_UNSUPPORTED_ACTIONS.contains(&action_id) {
            return Err(format!(
                "{WEB_HUMAN_REFUSAL_ACTION_UNSUPPORTED}:{action_id}"
            ));
        }
        // An unknown declared type fails closed before any value is interpreted.
        for field in card.fields.iter() {
            if !WEB_HUMAN_FIELD_TYPES.contains(&field.field_type.as_str()) {
                return Err("web_human_field_type_unknown".to_owned());
            }
        }
        if card.revoked {
            return Err("web_human_action_revoked".to_owned());
        }
        if card.is_expired(now_unix_ms) {
            return Err("web_human_action_expired".to_owned());
        }
        if card.expected_revision != observed_card_revision {
            return Err("web_human_action_revision_mismatch".to_owned());
        }
        if !card.allows(action_id) {
            return Err("web_human_decision_not_allowed".to_owned());
        }
        let Some(fields_object) = fields.as_object() else {
            return Err("web_human_fields_object_required".to_owned());
        };
        if fields_object.len() > 32
            || serde_json::to_vec(&fields)
                .map(|bytes| bytes.len() > WEB_HUMAN_MAX_FIELD_BYTES)
                .unwrap_or(true)
        {
            return Err("web_human_fields_limit".to_owned());
        }
        for field_name in fields_object.keys() {
            if !card.fields.iter().any(|field| field.name == *field_name) {
                // A hidden client field cannot widen the server action scope.
                return Err("web_human_field_denied".to_owned());
            }
        }
        for field in card.fields.iter().filter(|field| field.required) {
            if !fields_object.contains_key(&field.name) {
                return Err("web_human_required_field_missing".to_owned());
            }
        }
        for field in card.fields.iter() {
            if let Some(value) = fields_object.get(&field.name) {
                validate_field_value(field, value)?;
            }
        }
        Ok(UiHumanActionIntentV1 {
            schema: UI_HUMAN_ACTION_INTENT_SCHEMA.to_owned(),
            item_id: item_id.to_owned(),
            action_id: action_id.to_owned(),
            inbox_revision: self.revision.clone(),
            expected_revision: card.expected_revision,
            fields,
            idempotency_key: idempotency_key.to_owned(),
            payload_digest: card.payload_digest.clone(),
        })
    }
}
