//! Incremental backup chains, retention, legal hold and the backup deletion dependency graph.
//!
//! An incremental backup only makes sense as a chain: it stores the delta since a parent and
//! cannot be restored without it. This module owns that relationship. A chain may be archived,
//! expired or deleted, but never while a live child still depends on the parent it would remove,
//! and never while a legal hold covers the data. Deletion is therefore planned, not performed:
//! this module returns a plan and the evidence behind it, and the adapter carries it out.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, EventCursor, SchemaVersion, SecretRefId,
    SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const BACKUP_CHAIN_SCHEMA: &str = "kiana.backup-chain.v1";
pub const BACKUP_CHAIN_ENTRY_SCHEMA: &str = "kiana.backup-chain-entry.v1";
pub const BACKUP_HOLD_SCHEMA: &str = "kiana.backup-legal-hold.v1";
pub const BACKUP_DELETION_PLAN_SCHEMA: &str = "kiana.backup-deletion-plan.v1";
pub const BACKUP_LIFECYCLE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_BACKUP_CHAIN: usize = 256;

/// How a backup relates to the one before it.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupLink {
    /// A complete snapshot. Restorable on its own.
    Full,
    /// Stores only the delta since its parent, and is not restorable without it.
    Incremental,
    /// Moved out of the live store but still restorable.
    Archived,
}

impl BackupLink {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Incremental => "incremental",
            Self::Archived => "archived",
        }
    }
}

/// The lifecycle position of one backup in its chain.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupState {
    /// Available for restore.
    Live,
    /// Past its retention window, awaiting a deletion decision.
    Expired,
    /// Moved to archive storage.
    Archived,
    /// Removed. Terminal.
    Deleted,
    /// The adapter could not establish the state.
    Unknown,
}

impl BackupState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Expired => "expired",
            Self::Archived => "archived",
            Self::Deleted => "deleted",
            Self::Unknown => "unknown",
        }
    }
}

/// One backup's position in the chain, and what it needs to be restorable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupChainEntry {
    pub schema: String,
    pub version: SchemaVersion,
    pub backup_id: String,
    pub link: BackupLink,
    pub state: BackupState,
    /// The backup this one stores its delta against. A `Full` backup must not have one; an
    /// `Incremental` backup must have exactly one.
    pub parent_id: Option<String>,
    /// Cursor the parent already covered, for an incremental entry. Equal to the backup's own
    /// cursor for a full one.
    pub covered_from_cursor: EventCursor,
    pub source_cursor: EventCursor,
    pub projection_generation: u64,
    pub data_epoch: u64,
    /// Key reference for an encrypted backup. Required before the backup is restorable, and
    /// never an inline key: only the reference travels with the manifest.
    pub encryption_key_ref: Option<SecretRefId>,
    pub entry_digest: String,
}

impl BackupChainEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backup_id: impl Into<String>,
        link: BackupLink,
        state: BackupState,
        parent_id: Option<String>,
        covered_from_cursor: EventCursor,
        source_cursor: EventCursor,
        projection_generation: u64,
        data_epoch: u64,
        encryption_key_ref: Option<SecretRefId>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: BACKUP_CHAIN_ENTRY_SCHEMA.to_owned(),
            version: BACKUP_LIFECYCLE_VERSION,
            backup_id: backup_id.into(),
            link,
            state,
            parent_id,
            covered_from_cursor,
            source_cursor,
            projection_generation,
            data_epoch,
            encryption_key_ref,
            entry_digest: String::new(),
        };
        value.entry_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BACKUP_CHAIN_ENTRY_SCHEMA
            || self.version != BACKUP_LIFECYCLE_VERSION
            || self.source_cursor == 0
            || self.projection_generation == 0
            || self.data_epoch == 0
        {
            return Err("backup_chain_entry_header_invalid".to_owned());
        }
        safe_text(&self.backup_id, "backup_chain_backup_id")?;
        // A full backup is self-contained and must not claim a parent; an incremental one is
        // meaningless without one. Neither shape is negotiable, because getting it wrong makes
        // the chain unrestorable in a way that is only discovered at restore time.
        match (self.link, self.parent_id.is_some()) {
            (BackupLink::Full, true) => {
                return Err("backup_chain_full_with_parent".to_owned());
            }
            (BackupLink::Incremental, false) => {
                return Err("backup_chain_incremental_without_parent".to_owned());
            }
            _ => {}
        }
        if let Some(parent) = &self.parent_id {
            safe_text(parent, "backup_chain_parent_id")?;
            if parent == &self.backup_id {
                return Err("backup_chain_self_parent".to_owned());
            }
        }
        if self.covered_from_cursor > self.source_cursor {
            return Err("backup_chain_cursor_regression".to_owned());
        }
        // An incremental backup that claims to cover nothing new is not a delta; it is a copy
        // that would silently make the chain longer without adding a restore point.
        if self.link == BackupLink::Incremental && self.covered_from_cursor == self.source_cursor {
            return Err("backup_chain_incremental_covers_nothing".to_owned());
        }
        valid_digest(&self.entry_digest, "backup_chain_entry_digest")?;
        if self.entry_digest != self.digest() {
            return Err("backup_chain_entry_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// An encrypted backup is only restorable while its key reference resolves. The reference
    /// itself is opaque: a missing or nil one makes the backup unreadable, not merely awkward.
    pub fn restorable(&self) -> Result<(), String> {
        self.validate()?;
        if self.state == BackupState::Deleted {
            return Err("backup_chain_already_deleted".to_owned());
        }
        if self.state == BackupState::Unknown {
            return Err("backup_chain_state_unknown".to_owned());
        }
        if let Some(key) = &self.encryption_key_ref {
            if key.as_uuid().is_nil() {
                return Err("backup_chain_encryption_key_missing".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "backup_id": self.backup_id,
            "link": self.link,
            "state": self.state,
            "parent_id": self.parent_id,
            "covered_from_cursor": self.covered_from_cursor,
            "source_cursor": self.source_cursor,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "encryption_key_ref": self.encryption_key_ref,
        }))
    }
}

/// A legal hold. While one covers a backup, that backup cannot be deleted by retention.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupLegalHold {
    pub schema: String,
    pub version: SchemaVersion,
    pub hold_id: String,
    /// Backups this hold covers. An empty set would be a hold that silently protects nothing.
    pub backup_ids: Vec<String>,
    pub placed_by: String,
    pub reason: String,
    pub placed_at_unix_ms: u64,
    pub hold_digest: String,
}

impl BackupLegalHold {
    pub fn new(
        hold_id: impl Into<String>,
        backup_ids: Vec<String>,
        placed_by: impl Into<String>,
        reason: impl Into<String>,
        placed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: BACKUP_HOLD_SCHEMA.to_owned(),
            version: BACKUP_LIFECYCLE_VERSION,
            hold_id: hold_id.into(),
            backup_ids,
            placed_by: placed_by.into(),
            reason: reason.into(),
            placed_at_unix_ms,
            hold_digest: String::new(),
        };
        value.hold_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BACKUP_HOLD_SCHEMA
            || self.version != BACKUP_LIFECYCLE_VERSION
            || self.backup_ids.is_empty()
            || self.placed_at_unix_ms == 0
        {
            return Err("backup_legal_hold_header_invalid".to_owned());
        }
        safe_text(&self.hold_id, "backup_hold_id")?;
        safe_text(&self.placed_by, "backup_hold_placed_by")?;
        safe_text(&self.reason, "backup_hold_reason")?;
        let mut seen = BTreeSet::new();
        for backup_id in &self.backup_ids {
            safe_text(backup_id, "backup_hold_backup_id")?;
            if !seen.insert(backup_id) {
                return Err("backup_legal_hold_duplicate_backup".to_owned());
            }
        }
        valid_digest(&self.hold_digest, "backup_hold_digest")?;
        if self.hold_digest != self.digest() {
            return Err("backup_hold_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn covers(&self, backup_id: &str) -> bool {
        self.backup_ids.iter().any(|held| held == backup_id)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "hold_id": self.hold_id,
            "backup_ids": self.backup_ids,
            "placed_by": self.placed_by,
            "reason": self.reason,
            "placed_at_unix_ms": self.placed_at_unix_ms,
        }))
    }
}

/// What a retention pass is allowed to remove.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionMode {
    /// Remove only backups no live child depends on and no hold covers.
    Bounded,
    /// Report what would be removed without removing anything.
    DryRun,
}

impl DeletionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bounded => "bounded",
            Self::DryRun => "dry_run",
        }
    }
}

/// The ordered result of a retention pass over one chain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupDeletionPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub mode: DeletionMode,
    /// Backups that may be removed, parents before children.
    pub deletable: Vec<String>,
    /// Backups that were considered and refused, with the stable reason for each.
    pub retained: Vec<(String, String)>,
    /// Every hold that shaped this decision, so the plan can be audited against them.
    pub hold_ids: Vec<String>,
    pub chain_digest: String,
    pub plan_digest: String,
}

impl BackupDeletionPlan {
    /// Decide what retention may remove from a chain.
    ///
    /// Refusals are ordered before approvals, so a chain that is partly blocked still reports
    /// every reason rather than only the first. A parent is never deletable while a live child
    /// depends on it: removing it would leave that child unrestorable, and that failure only
    /// surfaces at restore time.
    pub fn plan(
        entries: &[BackupChainEntry],
        holds: &[BackupLegalHold],
        mode: DeletionMode,
    ) -> Result<Self, String> {
        let chain = BackupChain::new(entries.to_vec())?;
        let mut valid_holds = BTreeSet::new();
        for hold in holds {
            hold.validate()?;
            valid_holds.insert(hold.hold_id.clone());
        }

        // Which backups a live backup still needs in order to restore.
        let mut required: BTreeSet<String> = BTreeSet::new();
        for entry in &chain.entries {
            if entry.state != BackupState::Live && entry.state != BackupState::Archived {
                continue;
            }
            if let Some(parent) = &entry.parent_id {
                required.insert(parent.clone());
            }
        }

        let mut deletable = Vec::new();
        let mut retained = Vec::new();
        for entry in &chain.entries {
            if entry.state != BackupState::Expired {
                continue;
            }
            if let Some(hold) = holds.iter().find(|hold| hold.covers(&entry.backup_id)) {
                retained.push((
                    entry.backup_id.clone(),
                    format!("backup_hold_{}", hold.hold_id),
                ));
                continue;
            }
            if required.contains(&entry.backup_id) {
                retained.push((
                    entry.backup_id.clone(),
                    "backup_required_by_live_child".to_owned(),
                ));
                continue;
            }
            deletable.push(entry.backup_id.clone());
        }

        // A child can only be removed after its parent, or the chain would be left referring to
        // a backup that is already gone.
        deletable.sort_by_key(|backup_id| chain.depth_of(backup_id).unwrap_or(usize::MAX));
        deletable.reverse();

        let mut value = Self {
            schema: BACKUP_DELETION_PLAN_SCHEMA.to_owned(),
            version: BACKUP_LIFECYCLE_VERSION,
            mode,
            deletable: if mode == DeletionMode::DryRun {
                Vec::new()
            } else {
                deletable
            },
            retained,
            hold_ids: valid_holds.into_iter().collect(),
            chain_digest: chain.digest(),
            plan_digest: String::new(),
        };
        value.plan_digest = value.digest();
        Ok(value)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "mode": self.mode,
            "deletable": self.deletable,
            "retained": self.retained,
            "hold_ids": self.hold_ids,
            "chain_digest": self.chain_digest,
        }))
    }

    pub fn validate_against(&self, entries: &[BackupChainEntry]) -> Result<(), String> {
        let chain = BackupChain::new(entries.to_vec())?;
        if self.schema != BACKUP_DELETION_PLAN_SCHEMA
            || self.version != BACKUP_LIFECYCLE_VERSION
            || self.chain_digest != chain.digest()
        {
            return Err("backup_deletion_plan_chain_mismatch".to_owned());
        }
        // A dry run reports but never authorizes: it must not carry a removal list a caller
        // could act on by mistake.
        if self.mode == DeletionMode::DryRun && !self.deletable.is_empty() {
            return Err("backup_deletion_plan_dry_run_not_empty".to_owned());
        }
        valid_digest(&self.plan_digest, "backup_deletion_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("backup_deletion_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

/// An ordered set of backups forming one restore chain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupChain {
    pub schema: String,
    pub version: SchemaVersion,
    pub entries: Vec<BackupChainEntry>,
    pub chain_digest: String,
}

impl BackupChain {
    pub fn new(mut entries: Vec<BackupChainEntry>) -> Result<Self, String> {
        if entries.is_empty() || entries.len() > MAX_BACKUP_CHAIN {
            return Err("backup_chain_size_invalid".to_owned());
        }
        entries.sort_by(|left, right| left.source_cursor.cmp(&right.source_cursor));
        let mut value = Self {
            schema: BACKUP_CHAIN_SCHEMA.to_owned(),
            version: BACKUP_LIFECYCLE_VERSION,
            entries,
            chain_digest: String::new(),
        };
        value.chain_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BACKUP_CHAIN_SCHEMA || self.version != BACKUP_LIFECYCLE_VERSION {
            return Err("backup_chain_header_invalid".to_owned());
        }
        let mut by_id: BTreeMap<&str, &BackupChainEntry> = BTreeMap::new();
        for entry in &self.entries {
            entry.validate()?;
            if by_id.insert(entry.backup_id.as_str(), entry).is_some() {
                return Err("backup_chain_duplicate_backup".to_owned());
            }
        }
        // Cursors must strictly increase along the chain, and each incremental entry must
        // continue exactly where its parent stopped. A gap means data that no backup holds.
        let mut previous_cursor = 0;
        for entry in &self.entries {
            if entry.source_cursor <= previous_cursor {
                return Err("backup_chain_cursor_not_increasing".to_owned());
            }
            if let Some(parent_id) = &entry.parent_id {
                let Some(parent) = by_id.get(parent_id.as_str()) else {
                    return Err("backup_chain_parent_missing".to_owned());
                };
                if parent.covered_from_cursor != entry.covered_from_cursor {
                    return Err("backup_chain_parent_cursor_mismatch".to_owned());
                }
            }
            previous_cursor = entry.source_cursor;
        }
        // Every parent must precede its child, or the chain is a cycle.
        for entry in &self.entries {
            if let Some(parent_id) = &entry.parent_id {
                if let Some(parent) = by_id.get(parent_id.as_str()) {
                    if parent.source_cursor >= entry.source_cursor {
                        return Err("backup_chain_parent_after_child".to_owned());
                    }
                }
            }
        }
        valid_digest(&self.chain_digest, "backup_chain_digest")?;
        if self.chain_digest != self.digest() {
            return Err("backup_chain_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// How deep an entry sits in the chain; a full backup is at depth 0. `None` when the id is
    /// not in this chain.
    pub fn depth_of(&self, backup_id: &str) -> Option<usize> {
        let by_id: BTreeMap<&str, &BackupChainEntry> = self
            .entries
            .iter()
            .map(|entry| (entry.backup_id.as_str(), entry))
            .collect();
        let mut depth = 0;
        let mut current = by_id.get(backup_id)?;
        let mut seen = BTreeSet::new();
        while let Some(parent_id) = &current.parent_id {
            if !seen.insert(current.backup_id.clone()) {
                return None;
            }
            let parent = by_id.get(parent_id.as_str())?;
            depth += 1;
            current = parent;
        }
        Some(depth)
    }

    /// The backups that must be present, oldest first, to restore `backup_id`. A backup that
    /// cannot be reached through its parents is not restorable, whatever its own state says.
    pub fn restore_order(&self, backup_id: &str) -> Result<Vec<String>, String> {
        let by_id: BTreeMap<&str, &BackupChainEntry> = self
            .entries
            .iter()
            .map(|entry| (entry.backup_id.as_str(), entry))
            .collect();
        let mut order = Vec::new();
        let mut current = by_id.get(backup_id).ok_or("backup_chain_target_missing")?;
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(current.backup_id.clone()) {
                return Err("backup_chain_cycle".to_owned());
            }
            current.restorable()?;
            order.push(current.backup_id.clone());
            match &current.parent_id {
                Some(parent_id) => {
                    current = by_id
                        .get(parent_id.as_str())
                        .ok_or("backup_chain_parent_missing")?;
                }
                None => break,
            }
        }
        order.reverse();
        Ok(order)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "entries": self.entries,
        }))
    }
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > 256
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("://")
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
