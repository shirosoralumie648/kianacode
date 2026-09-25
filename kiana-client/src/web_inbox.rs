//! Typed Web Human Inbox and action-card presenter contracts.
//!
//! This module keeps only a bounded server projection and emits a versioned intent.  It never
//! chooses an actor, creates an approval, expands a scope or executes a command.  The Web server
//! and ControlPlane must recheck the owner lease, inbox/action revision, expiry, payload digest
//! and idempotency key when the intent crosses the transport boundary.

use crate::web_contract::WebClientTab;
use kiana_protocol::{
    UiHumanActionCardV1, UiHumanActionIntentV1, UiHumanInboxV1, UI_HUMAN_ACTION_INTENT_SCHEMA,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub const WEB_HUMAN_INBOX_SCHEMA: &str = "kiana.web-human-inbox.v1";
pub const WEB_HUMAN_MAX_CARDS: usize = 256;
pub const WEB_HUMAN_MAX_FIELD_BYTES: usize = 64 * 1024;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
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
