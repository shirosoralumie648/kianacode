//! Deterministic extension catalog selection and hook ordering.
//!
//! Catalog construction is a pure projection.  A selected entry is still only metadata; trust,
//! scope, activation and capability authorization remain separate ControlPlane decisions.

use crate::{json_digest, SourceRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const EXTENSION_CATALOG_SCHEMA: &str = "kiana.extension-catalog.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogEntryKind {
    Skill,
    Hook,
    Plugin,
}

impl CatalogEntryKind {
    fn rank(self) -> u8 {
        match self {
            Self::Skill => 0,
            Self::Hook => 1,
            Self::Plugin => 2,
        }
    }
}

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogCandidate {
    pub kind: CatalogEntryKind,
    pub namespace: String,
    pub name: String,
    pub version: String,
    pub content_digest: String,
    pub source: SourceRef,
    /// Lower values have higher precedence.  The value is source metadata, not authority.
    pub precedence: u16,
}

impl CatalogCandidate {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.namespace, "catalog_namespace", 128)?;
        required(&self.name, "catalog_name", 128)?;
        required(&self.version, "catalog_version", 64)?;
        digest(&self.content_digest, "catalog_content_digest")?;
        self.source.validate()
    }

    pub fn identity_key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.kind.rank(),
            self.namespace,
            self.name,
            self.version
        )
    }

    pub fn candidate_key(&self) -> String {
        json_digest(&serde_json::to_value(self).unwrap_or(Value::Null))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub candidate: CatalogCandidate,
    pub selected: bool,
    #[serde(default)]
    pub shadowed_by: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionCatalog {
    pub schema: String,
    pub generation: u64,
    pub entries: Vec<CatalogEntry>,
    pub catalog_digest: String,
}

impl ExtensionCatalog {
    pub fn new(generation: u64, candidates: Vec<CatalogCandidate>) -> Result<Self, String> {
        if generation == 0 || candidates.len() > 1_024 {
            return Err("extension_catalog_header_invalid".to_owned());
        }
        for candidate in &candidates {
            candidate.validate()?;
        }
        let mut candidates = candidates;
        candidates.sort_by(|left, right| {
            left.identity_key()
                .cmp(&right.identity_key())
                .then_with(|| left.precedence.cmp(&right.precedence))
                .then_with(|| left.source.source_id.cmp(&right.source.source_id))
                .then_with(|| left.content_digest.cmp(&right.content_digest))
        });
        let mut winners: BTreeMap<String, String> = BTreeMap::new();
        let mut entries = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let identity = candidate.identity_key();
            let candidate_key = candidate.candidate_key();
            if let Some(winner) = winners.get(&identity) {
                entries.push(CatalogEntry {
                    candidate,
                    selected: false,
                    shadowed_by: Some(winner.clone()),
                    reason: "duplicate_identity_shadowed".to_owned(),
                });
            } else {
                winners.insert(identity, candidate_key);
                entries.push(CatalogEntry {
                    candidate,
                    selected: true,
                    shadowed_by: None,
                    reason: "selected_by_precedence_then_source".to_owned(),
                });
            }
        }
        let mut catalog = Self {
            schema: EXTENSION_CATALOG_SCHEMA.to_owned(),
            generation,
            entries,
            catalog_digest: String::new(),
        };
        catalog.catalog_digest = catalog.digest();
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_CATALOG_SCHEMA
            || self.generation == 0
            || self.entries.len() > 1_024
        {
            return Err("extension_catalog_header_invalid".to_owned());
        }
        digest(&self.catalog_digest, "extension_catalog_digest")?;
        if self.catalog_digest != self.digest() {
            return Err("extension_catalog_digest_mismatch".to_owned());
        }
        let mut selected = BTreeSet::new();
        for entry in &self.entries {
            entry.candidate.validate()?;
            if entry.selected {
                if !selected.insert(entry.candidate.identity_key()) {
                    return Err("extension_catalog_identity_duplicate".to_owned());
                }
                if entry.shadowed_by.is_some()
                    || entry.reason != "selected_by_precedence_then_source"
                {
                    return Err("extension_catalog_selection_invalid".to_owned());
                }
            } else if entry.shadowed_by.is_none() || entry.reason != "duplicate_identity_shadowed" {
                return Err("extension_catalog_shadow_invalid".to_owned());
            }
        }
        Ok(())
    }

    pub fn selected(&self) -> impl Iterator<Item = &CatalogCandidate> {
        self.entries
            .iter()
            .filter(|entry| entry.selected)
            .map(|entry| &entry.candidate)
    }

    pub fn shadowed(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.entries.iter().filter(|entry| !entry.selected)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("catalog_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookOrderCandidate {
    pub hook_id: String,
    pub event: String,
    pub matcher: String,
    pub source_id: String,
    pub precedence: u16,
}

impl HookOrderCandidate {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.hook_id, "hook_order_id", 256)?;
        required(&self.event, "hook_order_event", 128)?;
        required(&self.matcher, "hook_order_matcher", 1_024)?;
        required(&self.source_id, "hook_order_source", 256)
    }

    pub fn specificity(&self) -> usize {
        self.matcher
            .chars()
            .filter(|character| !matches!(character, '*' | '?' | '[' | ']'))
            .count()
    }
}

/// Sort matched hooks without relying on filesystem or HashMap iteration order.
pub fn order_hooks(mut hooks: Vec<HookOrderCandidate>) -> Result<Vec<HookOrderCandidate>, String> {
    let mut ids = BTreeSet::new();
    for hook in &hooks {
        hook.validate()?;
        if !ids.insert(hook.hook_id.clone()) {
            return Err("hook_order_duplicate_id".to_owned());
        }
    }
    hooks.sort_by(|left, right| {
        left.precedence
            .cmp(&right.precedence)
            .then_with(|| right.specificity().cmp(&left.specificity()))
            .then_with(|| left.event.cmp(&right.event))
            .then_with(|| left.matcher.cmp(&right.matcher))
            .then_with(|| left.hook_id.cmp(&right.hook_id))
            .then_with(|| left.source_id.cmp(&right.source_id))
    });
    Ok(hooks)
}
