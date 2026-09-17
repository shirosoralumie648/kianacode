//! Root/child grant authority envelopes and an append-only in-memory ledger contract.
//!
//! The ledger is a pure domain reducer used by ControlPlane adapters. A grant is bounded by its
//! parent and cannot be transferred; revoking an ancestor fences every descendant. Persistence and
//! effect-time permit consumption remain above this module.

use crate::{json_digest, AuthenticatedPrincipalRef, GrantId, ProjectId, SchemaVersion, ScopeSet};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const GRANT_AUTHORITY_SCHEMA: &str = "kiana.grant-authority.v1";
pub const GRANT_LEDGER_SNAPSHOT_SCHEMA: &str = "kiana.grant-ledger-snapshot.v1";
pub const GRANT_AUTHORITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantAuthorityStatus {
    Active,
    Revoked,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantAuthorityEnvelope {
    pub schema: String,
    pub version: SchemaVersion,
    pub grant_id: GrantId,
    #[serde(default)]
    pub parent_grant_id: Option<GrantId>,
    pub principal: AuthenticatedPrincipalRef,
    pub project_id: ProjectId,
    pub issuer: AuthenticatedPrincipalRef,
    pub root_run: bool,
    pub scope: ScopeSet,
    pub authority_epoch: u64,
    pub revision: u64,
    pub expires_at_unix_ms: u64,
    pub status: GrantAuthorityStatus,
    #[serde(default)]
    pub revoked_at_epoch: Option<u64>,
    pub grant_digest: String,
}

impl GrantAuthorityEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        grant_id: GrantId,
        parent_grant_id: Option<GrantId>,
        principal: AuthenticatedPrincipalRef,
        project_id: ProjectId,
        issuer: AuthenticatedPrincipalRef,
        root_run: bool,
        scope: ScopeSet,
        authority_epoch: u64,
        revision: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut grant = Self {
            schema: GRANT_AUTHORITY_SCHEMA.to_owned(),
            version: GRANT_AUTHORITY_VERSION,
            grant_id,
            parent_grant_id,
            principal,
            project_id,
            issuer,
            root_run,
            scope,
            authority_epoch,
            revision,
            expires_at_unix_ms,
            status: GrantAuthorityStatus::Active,
            revoked_at_epoch: None,
            grant_digest: String::new(),
        };
        grant.grant_digest = grant.digest();
        grant.validate()?;
        Ok(grant)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let grant: Self = serde_json::from_value(value.clone())
            .map_err(|_| "grant_authority_decode_failed".to_owned())?;
        grant.validate()?;
        Ok(grant)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "grant_authority_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != GRANT_AUTHORITY_SCHEMA
            || !self.version.is_compatible_with(&GRANT_AUTHORITY_VERSION)
            || self.grant_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.revision == 0
            || self.expires_at_unix_ms == 0
        {
            return Err("grant_authority_header_invalid".to_owned());
        }
        if self.parent_grant_id == Some(self.grant_id) {
            return Err("grant_authority_parent_self".to_owned());
        }
        self.principal.validate()?;
        self.issuer.validate()?;
        self.scope.validate()?;
        if self.root_run {
            if self.parent_grant_id.is_some() {
                return Err("grant_authority_root_parent_unexpected".to_owned());
            }
            if self.scope.operations == crate::ScopeDimension::NotApplicable
                || self.scope.paths == crate::ScopeDimension::NotApplicable
            {
                return Err("grant_authority_root_scope_unbounded".to_owned());
            }
        } else if self.parent_grant_id.is_none() {
            return Err("grant_authority_parent_required".to_owned());
        }
        match (self.status, self.revoked_at_epoch) {
            (GrantAuthorityStatus::Active, Some(_)) => {
                return Err("grant_authority_revocation_unexpected".to_owned())
            }
            (GrantAuthorityStatus::Revoked, Some(epoch)) if epoch < self.authority_epoch => {
                return Err("grant_authority_revocation_epoch_invalid".to_owned())
            }
            (GrantAuthorityStatus::Revoked, None) => {
                return Err("grant_authority_revocation_epoch_required".to_owned())
            }
            _ => {}
        }
        validate_digest(&self.grant_digest, "grant_authority_digest")?;
        if self.grant_digest != self.digest() {
            return Err("grant_authority_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn revoke(&self, authority_epoch: u64) -> Result<Self, String> {
        self.validate()?;
        if authority_epoch < self.authority_epoch {
            return Err("grant_authority_epoch_rollback".to_owned());
        }
        if self.status == GrantAuthorityStatus::Revoked {
            return Err("grant_authority_already_revoked".to_owned());
        }
        let mut next = self.clone();
        next.status = GrantAuthorityStatus::Revoked;
        next.authority_epoch = authority_epoch;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "grant_authority_revision_exhausted".to_owned())?;
        next.revoked_at_epoch = Some(authority_epoch);
        next.grant_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn active_at(&self, now_unix_ms: u64, authority_epoch: u64) -> bool {
        self.status == GrantAuthorityStatus::Active
            && self.authority_epoch == authority_epoch
            && now_unix_ms < self.expires_at_unix_ms
    }

    /// Check that a child is a strict descendant bounded by this grant.
    pub fn contains(&self, child: &Self) -> Result<bool, String> {
        self.validate()?;
        child.validate()?;
        Ok(self.status == GrantAuthorityStatus::Active
            && child.status == GrantAuthorityStatus::Active
            && child.parent_grant_id == Some(self.grant_id)
            && child.principal == self.principal
            && child.project_id == self.project_id
            && child.issuer == self.issuer
            && child.authority_epoch == self.authority_epoch
            && child.expires_at_unix_ms <= self.expires_at_unix_ms
            && child.scope.is_subset_of(&self.scope)?)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "grant_id": self.grant_id,
            "parent_grant_id": self.parent_grant_id,
            "principal": self.principal,
            "project_id": self.project_id,
            "issuer": self.issuer,
            "root_run": self.root_run,
            "scope": self.scope,
            "authority_epoch": self.authority_epoch,
            "revision": self.revision,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "status": self.status,
            "revoked_at_epoch": self.revoked_at_epoch,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantLedgerSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub authority_epoch: u64,
    pub source_sequence: u64,
    pub grants: Vec<GrantAuthorityEnvelope>,
    pub snapshot_digest: String,
}

impl GrantLedgerSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != GRANT_LEDGER_SNAPSHOT_SCHEMA
            || !self.version.is_compatible_with(&GRANT_AUTHORITY_VERSION)
            || self.authority_epoch == 0
            || self.source_sequence == 0
            || self.grants.len() > 512
        {
            return Err("grant_ledger_snapshot_header_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for grant in &self.grants {
            grant.validate()?;
            if !ids.insert(grant.grant_id) {
                return Err("grant_ledger_duplicate_id".to_owned());
            }
            if grant.authority_epoch > self.authority_epoch {
                return Err("grant_ledger_epoch_ahead".to_owned());
            }
        }
        if self
            .grants
            .windows(2)
            .any(|pair| pair[0].grant_id.as_uuid() >= pair[1].grant_id.as_uuid())
        {
            return Err("grant_ledger_grants_noncanonical".to_owned());
        }
        validate_digest(&self.snapshot_digest, "grant_ledger_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("grant_ledger_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "authority_epoch": self.authority_epoch,
            "source_sequence": self.source_sequence,
            "grants": self.grants,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GrantLedger {
    authority_epoch: u64,
    source_sequence: u64,
    grants: BTreeMap<GrantId, GrantAuthorityEnvelope>,
}

impl GrantLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(&mut self, grant: GrantAuthorityEnvelope, sequence: u64) -> Result<(), String> {
        if sequence != self.source_sequence.saturating_add(1) {
            return Err("grant_ledger_sequence_gap_or_regression".to_owned());
        }
        grant.validate()?;
        if self.grants.contains_key(&grant.grant_id) {
            return Err("grant_ledger_duplicate_id".to_owned());
        }
        if grant.authority_epoch < self.authority_epoch {
            return Err("grant_ledger_epoch_rollback".to_owned());
        }
        if let Some(parent_id) = grant.parent_grant_id {
            let parent = self
                .grants
                .get(&parent_id)
                .ok_or_else(|| "grant_ledger_parent_missing".to_owned())?;
            if !parent.contains(&grant)? {
                return Err("grant_ledger_child_scope_widened".to_owned());
            }
        }
        self.authority_epoch = self.authority_epoch.max(grant.authority_epoch);
        self.source_sequence = sequence;
        self.grants.insert(grant.grant_id, grant);
        Ok(())
    }

    pub fn revoke(
        &mut self,
        grant_id: GrantId,
        authority_epoch: u64,
        sequence: u64,
    ) -> Result<(), String> {
        if sequence != self.source_sequence.saturating_add(1) {
            return Err("grant_ledger_sequence_gap_or_regression".to_owned());
        }
        if authority_epoch < self.authority_epoch {
            return Err("grant_ledger_epoch_rollback".to_owned());
        }
        let grant = self
            .grants
            .get(&grant_id)
            .cloned()
            .ok_or_else(|| "grant_ledger_grant_missing".to_owned())?;
        let revoked = grant.revoke(authority_epoch)?;
        self.grants.insert(grant_id, revoked);
        self.authority_epoch = authority_epoch;
        self.source_sequence = sequence;
        Ok(())
    }

    pub fn active_grant(
        &self,
        grant_id: GrantId,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<bool, String> {
        let grant = self
            .grants
            .get(&grant_id)
            .ok_or_else(|| "grant_ledger_grant_missing".to_owned())?;
        if !grant.active_at(now_unix_ms, authority_epoch) {
            return Ok(false);
        }
        let mut current = grant.parent_grant_id;
        while let Some(parent_id) = current {
            let parent = self
                .grants
                .get(&parent_id)
                .ok_or_else(|| "grant_ledger_parent_missing".to_owned())?;
            if !parent.active_at(now_unix_ms, authority_epoch) {
                return Ok(false);
            }
            current = parent.parent_grant_id;
        }
        Ok(true)
    }

    pub fn snapshot(&self) -> Result<GrantLedgerSnapshot, String> {
        if self.authority_epoch == 0 || self.source_sequence == 0 {
            return Err("grant_ledger_empty".to_owned());
        }
        let mut grants = self.grants.values().cloned().collect::<Vec<_>>();
        grants.sort_by_key(|grant| grant.grant_id.as_uuid());
        let mut snapshot = GrantLedgerSnapshot {
            schema: GRANT_LEDGER_SNAPSHOT_SCHEMA.to_owned(),
            version: GRANT_AUTHORITY_VERSION,
            authority_epoch: self.authority_epoch,
            source_sequence: self.source_sequence,
            grants,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn restore(snapshot: GrantLedgerSnapshot) -> Result<Self, String> {
        snapshot.validate()?;
        let mut ledger = Self {
            authority_epoch: snapshot.authority_epoch,
            source_sequence: snapshot.source_sequence,
            grants: BTreeMap::new(),
        };
        let mut pending = snapshot.grants;
        while !pending.is_empty() {
            let mut progressed = false;
            let mut index = 0;
            while index < pending.len() {
                let grant = &pending[index];
                let ready = grant
                    .parent_grant_id
                    .is_none_or(|parent| ledger.grants.contains_key(&parent));
                if !ready {
                    index += 1;
                    continue;
                }
                let grant = pending.remove(index);
                if let Some(parent) = grant.parent_grant_id {
                    let parent_grant = ledger
                        .grants
                        .get(&parent)
                        .ok_or_else(|| "grant_ledger_parent_missing".to_owned())?;
                    if parent_grant.principal != grant.principal
                        || parent_grant.project_id != grant.project_id
                        || parent_grant.issuer != grant.issuer
                        || (parent_grant.status == GrantAuthorityStatus::Active
                            && grant.authority_epoch != parent_grant.authority_epoch)
                        || (parent_grant.status == GrantAuthorityStatus::Revoked
                            && grant.authority_epoch > parent_grant.authority_epoch)
                        || grant.expires_at_unix_ms > parent_grant.expires_at_unix_ms
                        || !grant.scope.is_subset_of(&parent_grant.scope)?
                    {
                        return Err("grant_ledger_child_scope_widened".to_owned());
                    }
                }
                ledger.grants.insert(grant.grant_id, grant);
                progressed = true;
            }
            if !progressed {
                return Err("grant_ledger_parent_missing".to_owned());
            }
        }
        Ok(ledger)
    }

    pub fn authority_epoch(&self) -> u64 {
        self.authority_epoch
    }

    pub fn source_sequence(&self) -> u64 {
        self.source_sequence
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
