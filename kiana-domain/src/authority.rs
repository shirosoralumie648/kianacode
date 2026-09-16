//! Event-replayable authority ledger for Membership, assignments and bounded sharing.
//!
//! The ledger is a pure reducer over committed EventStore facts. It does not execute capabilities
//! or replace ControlPlane admission; it only provides a deterministic, epoch-fenced authority
//! snapshot for callers that have already authenticated a principal.
use crate::{
    json_digest, DataBoundaryId, Membership, PolicyProfileId, ProjectAssignment, ProjectId,
    RoleAssignment, RuntimeEvent, SharingGrantId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const POLICY_PROFILE_SCHEMA: &str = "kiana.policy-profile.v1";
pub const DATA_BOUNDARY_SCHEMA: &str = "kiana.data-boundary.v1";
pub const SHARING_GRANT_SCHEMA: &str = "kiana.sharing-grant.v1";
pub const AUTHORITY_LEDGER_SCHEMA: &str = "kiana.authority-ledger.v1";

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

fn list(values: &[String], field: &str, max: usize) -> Result<(), String> {
    if values.len() > max
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 512 || value.contains('\0'))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyProfileStatus {
    Active,
    Suspended,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyProfile {
    pub schema: String,
    pub profile_id: PolicyProfileId,
    pub version: u64,
    pub allowed_operations: Vec<String>,
    pub data_boundary_id: DataBoundaryId,
    pub authority_epoch: u64,
    pub status: PolicyProfileStatus,
    pub profile_digest: String,
}

impl PolicyProfile {
    pub fn new(
        profile_id: PolicyProfileId,
        allowed_operations: Vec<String>,
        data_boundary_id: DataBoundaryId,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut profile = Self {
            schema: POLICY_PROFILE_SCHEMA.to_owned(),
            profile_id,
            version: 1,
            allowed_operations,
            data_boundary_id,
            authority_epoch,
            status: PolicyProfileStatus::Active,
            profile_digest: String::new(),
        };
        profile.allowed_operations.sort();
        profile.allowed_operations.dedup();
        profile.profile_digest = profile.digest();
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != POLICY_PROFILE_SCHEMA || self.version == 0 || self.authority_epoch == 0 {
            return Err("policy_profile_header_invalid".to_owned());
        }
        list(&self.allowed_operations, "policy_profile_operations", 256)?;
        if self
            .allowed_operations
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err("policy_profile_operations_noncanonical".to_owned());
        }
        digest(&self.profile_digest, "policy_profile_digest")?;
        if self.profile_digest != self.digest() {
            return Err("policy_profile_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn allows(&self, operation: &str) -> bool {
        self.status == PolicyProfileStatus::Active
            && self
                .allowed_operations
                .iter()
                .any(|candidate| candidate == operation)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "profile_id": self.profile_id,
            "version": self.version,
            "allowed_operations": self.allowed_operations,
            "data_boundary_id": self.data_boundary_id,
            "authority_epoch": self.authority_epoch,
            "status": self.status,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataBoundary {
    pub schema: String,
    pub boundary_id: DataBoundaryId,
    pub project_ids: Vec<ProjectId>,
    pub data_classes: Vec<String>,
    pub allow_external: bool,
    pub authority_epoch: u64,
    pub boundary_digest: String,
}

impl DataBoundary {
    pub fn new(
        boundary_id: DataBoundaryId,
        project_ids: Vec<ProjectId>,
        data_classes: Vec<String>,
        allow_external: bool,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut boundary = Self {
            schema: DATA_BOUNDARY_SCHEMA.to_owned(),
            boundary_id,
            project_ids,
            data_classes,
            allow_external,
            authority_epoch,
            boundary_digest: String::new(),
        };
        boundary.project_ids.sort_by_key(|id| id.as_uuid());
        boundary.data_classes.sort();
        boundary.data_classes.dedup();
        boundary.boundary_digest = boundary.digest();
        boundary.validate()?;
        Ok(boundary)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DATA_BOUNDARY_SCHEMA || self.authority_epoch == 0 {
            return Err("data_boundary_header_invalid".to_owned());
        }
        if self.project_ids.is_empty()
            || self
                .project_ids
                .windows(2)
                .any(|pair| pair[0].as_uuid() >= pair[1].as_uuid())
        {
            return Err("data_boundary_projects_invalid".to_owned());
        }
        list(&self.data_classes, "data_boundary_classes", 128)?;
        if self.data_classes.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("data_boundary_classes_noncanonical".to_owned());
        }
        digest(&self.boundary_digest, "data_boundary_digest")?;
        if self.boundary_digest != self.digest() {
            return Err("data_boundary_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn contains_project(&self, project_id: ProjectId) -> bool {
        self.project_ids
            .binary_search_by_key(&project_id.as_uuid(), |id| id.as_uuid())
            .is_ok()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "boundary_id": self.boundary_id,
            "project_ids": self.project_ids,
            "data_classes": self.data_classes,
            "allow_external": self.allow_external,
            "authority_epoch": self.authority_epoch,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharingGrant {
    pub schema: String,
    pub grant_id: SharingGrantId,
    pub source_project: ProjectId,
    pub target_project: ProjectId,
    pub scope: Vec<String>,
    pub purpose: String,
    pub operations: Vec<String>,
    pub expires_at_unix_ms: u64,
    pub authority_epoch: u64,
    pub revoked: bool,
    pub grant_digest: String,
}

impl SharingGrant {
    pub fn new(
        grant_id: SharingGrantId,
        source_project: ProjectId,
        target_project: ProjectId,
        scope: Vec<String>,
        purpose: impl Into<String>,
        operations: Vec<String>,
        expires_at_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut grant = Self {
            schema: SHARING_GRANT_SCHEMA.to_owned(),
            grant_id,
            source_project,
            target_project,
            scope,
            purpose: purpose.into(),
            operations,
            expires_at_unix_ms,
            authority_epoch,
            revoked: false,
            grant_digest: String::new(),
        };
        grant.scope.sort();
        grant.scope.dedup();
        grant.operations.sort();
        grant.operations.dedup();
        grant.grant_digest = grant.digest();
        grant.validate()?;
        Ok(grant)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SHARING_GRANT_SCHEMA
            || self.source_project == self.target_project
            || self.expires_at_unix_ms == 0
            || self.authority_epoch == 0
        {
            return Err("sharing_grant_header_invalid".to_owned());
        }
        required(&self.purpose, "sharing_grant_purpose", 512)?;
        list(&self.scope, "sharing_grant_scope", 256)?;
        list(&self.operations, "sharing_grant_operations", 256)?;
        if self.scope.windows(2).any(|pair| pair[0] >= pair[1])
            || self.operations.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("sharing_grant_lists_noncanonical".to_owned());
        }
        digest(&self.grant_digest, "sharing_grant_digest")?;
        if self.grant_digest != self.digest() {
            return Err("sharing_grant_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn active_at(&self, now_unix_ms: u64, authority_epoch: u64) -> bool {
        !self.revoked
            && self.authority_epoch == authority_epoch
            && now_unix_ms < self.expires_at_unix_ms
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "grant_id": self.grant_id,
            "source_project": self.source_project,
            "target_project": self.target_project,
            "scope": self.scope,
            "purpose": self.purpose,
            "operations": self.operations,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "authority_epoch": self.authority_epoch,
            "revoked": self.revoked,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuthorityLedger {
    pub authority_epoch: u64,
    pub stream_version: u64,
    memberships: BTreeMap<String, Membership>,
    role_assignments: BTreeMap<String, RoleAssignment>,
    project_assignments: BTreeMap<String, ProjectAssignment>,
    policy_profiles: BTreeMap<String, PolicyProfile>,
    data_boundaries: BTreeMap<String, DataBoundary>,
    sharing_grants: BTreeMap<String, SharingGrant>,
}

impl AuthorityLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rebuild(events: &[RuntimeEvent]) -> Result<Self, String> {
        let mut ledger = Self::new();
        for event in events {
            ledger.apply_event(event)?;
        }
        Ok(ledger)
    }

    pub fn apply_event(&mut self, event: &RuntimeEvent) -> Result<(), String> {
        let version = event
            .stream_version
            .ok_or_else(|| "authority_event_version_required".to_owned())?;
        if version != self.stream_version.saturating_add(1) {
            return Err("authority_event_version_gap_or_regression".to_owned());
        }
        match event.kind.as_str() {
            "authority.revised" => {
                let epoch = version;
                if epoch < self.authority_epoch {
                    return Err("authority_epoch_rollback".to_owned());
                }
                self.authority_epoch = epoch;
            }
            "membership.bound" => {
                let membership: Membership = decode(&event.data, "membership")?;
                self.advance_epoch(membership.authority_epoch)?;
                Self::insert_unique(
                    &mut self.memberships,
                    membership.membership_id.to_string(),
                    membership,
                    "membership_duplicate",
                )?;
            }
            "role_assignment.bound" => {
                let assignment: RoleAssignment = decode(&event.data, "assignment")?;
                self.advance_epoch(assignment.authority_epoch)?;
                Self::insert_unique(
                    &mut self.role_assignments,
                    assignment.assignment_id.to_string(),
                    assignment,
                    "role_assignment_duplicate",
                )?;
            }
            "project_assignment.bound" => {
                let assignment: ProjectAssignment = decode(&event.data, "assignment")?;
                self.advance_epoch(assignment.authority_epoch)?;
                if !self
                    .role_assignments
                    .contains_key(&assignment.role_assignment_id.to_string())
                {
                    return Err("authority_role_assignment_missing".to_owned());
                }
                Self::insert_unique(
                    &mut self.project_assignments,
                    assignment.project_assignment_id.to_string(),
                    assignment,
                    "project_assignment_duplicate",
                )?;
            }
            "policy_profile.bound" => {
                let profile: PolicyProfile = decode(&event.data, "profile")?;
                self.advance_epoch(profile.authority_epoch)?;
                Self::insert_unique(
                    &mut self.policy_profiles,
                    profile.profile_id.to_string(),
                    profile,
                    "policy_profile_duplicate",
                )?;
            }
            "data_boundary.bound" => {
                let boundary: DataBoundary = decode(&event.data, "boundary")?;
                self.advance_epoch(boundary.authority_epoch)?;
                Self::insert_unique(
                    &mut self.data_boundaries,
                    boundary.boundary_id.to_string(),
                    boundary,
                    "data_boundary_duplicate",
                )?;
            }
            "sharing_grant.bound" => {
                let grant: SharingGrant = decode(&event.data, "grant")?;
                self.advance_epoch(grant.authority_epoch)?;
                Self::insert_unique(
                    &mut self.sharing_grants,
                    grant.grant_id.to_string(),
                    grant,
                    "sharing_grant_duplicate",
                )?;
            }
            "sharing_grant.revoked" => {
                let id = event.data["grant_id"]
                    .as_str()
                    .ok_or_else(|| "sharing_grant_id_required".to_owned())?;
                let grant = self
                    .sharing_grants
                    .get_mut(id)
                    .ok_or_else(|| "sharing_grant_missing".to_owned())?;
                let epoch = event.data["authority_epoch"]
                    .as_u64()
                    .ok_or_else(|| "authority_epoch_required".to_owned())?;
                if epoch < self.authority_epoch {
                    return Err("authority_epoch_rollback".to_owned());
                }
                self.authority_epoch = epoch;
                grant.revoked = true;
                grant.authority_epoch = epoch;
                grant.grant_digest = grant.digest();
                grant.validate()?;
            }
            _ => return Err("authority_event_kind_unknown".to_owned()),
        }
        self.stream_version = version;
        Ok(())
    }

    fn advance_epoch(&mut self, epoch: u64) -> Result<(), String> {
        if epoch == 0 {
            return Err("authority_epoch_invalid".to_owned());
        }
        if epoch < self.authority_epoch {
            return Err("authority_epoch_stale".to_owned());
        }
        self.authority_epoch = epoch;
        Ok(())
    }

    fn insert_unique<T>(
        map: &mut BTreeMap<String, T>,
        key: String,
        value: T,
        duplicate: &str,
    ) -> Result<(), String> {
        if map.insert(key, value).is_some() {
            return Err(duplicate.to_owned());
        }
        Ok(())
    }

    pub fn memberships(&self) -> impl Iterator<Item = &Membership> {
        self.memberships.values()
    }

    pub fn role_assignments(&self) -> impl Iterator<Item = &RoleAssignment> {
        self.role_assignments.values()
    }

    pub fn project_assignments(&self) -> impl Iterator<Item = &ProjectAssignment> {
        self.project_assignments.values()
    }

    pub fn policy_profiles(&self) -> impl Iterator<Item = &PolicyProfile> {
        self.policy_profiles.values()
    }

    pub fn data_boundaries(&self) -> impl Iterator<Item = &DataBoundary> {
        self.data_boundaries.values()
    }

    pub fn sharing_grants(&self) -> impl Iterator<Item = &SharingGrant> {
        self.sharing_grants.values()
    }

    pub fn sharing_operations(
        &self,
        source_project: ProjectId,
        target_project: ProjectId,
        now_unix_ms: u64,
    ) -> BTreeSet<String> {
        self.sharing_grants
            .values()
            .filter(|grant| {
                grant.source_project == source_project
                    && grant.target_project == target_project
                    && grant.active_at(now_unix_ms, self.authority_epoch)
            })
            .flat_map(|grant| grant.operations.iter().cloned())
            .collect()
    }

    pub fn snapshot_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": AUTHORITY_LEDGER_SCHEMA,
            "authority_epoch": self.authority_epoch,
            "stream_version": self.stream_version,
            "memberships": self.memberships,
            "role_assignments": self.role_assignments,
            "project_assignments": self.project_assignments,
            "policy_profiles": self.policy_profiles,
            "data_boundaries": self.data_boundaries,
            "sharing_grants": self.sharing_grants,
        }))
    }
}

fn decode<T: for<'de> Deserialize<'de>>(data: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(
        data.get(key)
            .cloned()
            .ok_or_else(|| format!("authority_{key}_required"))?,
    )
    .map_err(|_| format!("authority_{key}_invalid"))
}
