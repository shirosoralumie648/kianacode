//! Pure ProjectTrust root resolution for Skills, Plugins, Hooks and MCP resources.
//!
//! The policy layer does not inspect paths or load resources. An adapter supplies canonical root
//! and project digests, and this module selects a same-scope trust record before any loader may
//! read a project-local resource. Unknown, conflicting and untrusted roots are deny decisions.

use kiana_domain::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const PROJECT_TRUST_ROOT_SCHEMA: &str = "kiana.project-trust-root.v1";
pub const PROJECT_TRUST_RESOLUTION_SCHEMA: &str = "kiana.project-trust-resolution.v1";
pub const PROJECT_TRUST_POLICY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_PROJECT_TRUST_ROOTS: usize = 64;

fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTrustScope {
    User,
    KianaHome,
    Project,
}

impl ProjectTrustScope {
    pub const fn priority(self) -> u8 {
        match self {
            Self::User => 10,
            Self::KianaHome => 20,
            Self::Project => 30,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTrustState {
    Trusted,
    Untrusted,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTrustDecision {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrustRoot {
    pub schema: String,
    pub version: SchemaVersion,
    pub scope: ProjectTrustScope,
    pub project_root_digest: String,
    pub root_digest: String,
    pub revision: u64,
    pub state: ProjectTrustState,
    /// Stable redacted source/audit reference; never a raw path or trust-file payload.
    pub audit_ref: String,
    pub root_record_digest: String,
}

impl ProjectTrustRoot {
    pub fn new(
        scope: ProjectTrustScope,
        project_root_digest: impl Into<String>,
        root_digest: impl Into<String>,
        revision: u64,
        state: ProjectTrustState,
        audit_ref: impl Into<String>,
    ) -> Result<Self, String> {
        let mut root = Self {
            schema: PROJECT_TRUST_ROOT_SCHEMA.to_owned(),
            version: PROJECT_TRUST_POLICY_VERSION,
            scope,
            project_root_digest: project_root_digest.into(),
            root_digest: root_digest.into(),
            revision,
            state,
            audit_ref: audit_ref.into(),
            root_record_digest: String::new(),
        };
        root.root_record_digest = root.digest();
        root.validate()?;
        Ok(root)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_TRUST_ROOT_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECT_TRUST_POLICY_VERSION)
            || self.revision == 0
        {
            return Err("project_trust_root_header_invalid".to_owned());
        }
        valid_digest(&self.project_root_digest, "project_trust_project_digest")?;
        valid_digest(&self.root_digest, "project_trust_root_digest")?;
        nonempty(&self.audit_ref, "project_trust_audit_ref", 256)?;
        valid_digest(&self.root_record_digest, "project_trust_record_digest")?;
        if self.root_record_digest != self.digest() {
            return Err("project_trust_root_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "scope": self.scope,
            "project_root_digest": self.project_root_digest,
            "root_digest": self.root_digest,
            "revision": self.revision,
            "state": self.state,
            "audit_ref": self.audit_ref,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrustResolution {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_root_digest: String,
    pub requested_scope: ProjectTrustScope,
    pub candidates: Vec<ProjectTrustRoot>,
    pub selected_scope: Option<ProjectTrustScope>,
    pub selected_root_digest: Option<String>,
    pub decision: ProjectTrustDecision,
    pub reason: String,
    pub resolution_digest: String,
}

impl ProjectTrustResolution {
    pub fn resolve(
        project_root_digest: impl Into<String>,
        requested_scope: ProjectTrustScope,
        roots: Vec<ProjectTrustRoot>,
    ) -> Result<Self, String> {
        let project_root_digest = project_root_digest.into();
        valid_digest(&project_root_digest, "project_trust_project_digest")?;
        if roots.len() > MAX_PROJECT_TRUST_ROOTS {
            return Err("project_trust_root_limit".to_owned());
        }
        for root in &roots {
            root.validate()?;
        }
        let mut candidates = roots
            .into_iter()
            .filter(|root| {
                root.scope == requested_scope && root.project_root_digest == project_root_digest
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .revision
                .cmp(&left.revision)
                .then_with(|| right.scope.priority().cmp(&left.scope.priority()))
                .then_with(|| left.root_digest.cmp(&right.root_digest))
        });

        let (selected_scope, selected_root_digest, decision, reason) = if candidates.is_empty() {
            (
                None,
                None,
                ProjectTrustDecision::Deny,
                "project_trust_root_missing".to_owned(),
            )
        } else {
            let selected = &candidates[0];
            let states = candidates
                .iter()
                .filter(|root| root.revision == selected.revision)
                .map(|root| root.state)
                .collect::<BTreeSet<_>>();
            if states.len() > 1 {
                (
                    None,
                    None,
                    ProjectTrustDecision::Deny,
                    "project_trust_conflict".to_owned(),
                )
            } else {
                let decision = match selected.state {
                    ProjectTrustState::Trusted => ProjectTrustDecision::Allow,
                    ProjectTrustState::Untrusted => ProjectTrustDecision::Deny,
                    ProjectTrustState::Unknown => ProjectTrustDecision::Deny,
                };
                let reason = match selected.state {
                    ProjectTrustState::Trusted => "project_trust_allowed",
                    ProjectTrustState::Untrusted => "project_trust_untrusted",
                    ProjectTrustState::Unknown => "project_trust_unknown",
                };
                (
                    Some(selected.scope),
                    Some(selected.root_digest.clone()),
                    decision,
                    reason.to_owned(),
                )
            }
        };
        let mut resolution = Self {
            schema: PROJECT_TRUST_RESOLUTION_SCHEMA.to_owned(),
            version: PROJECT_TRUST_POLICY_VERSION,
            project_root_digest,
            requested_scope,
            candidates,
            selected_scope,
            selected_root_digest,
            decision,
            reason,
            resolution_digest: String::new(),
        };
        resolution.resolution_digest = resolution.digest();
        resolution.validate()?;
        Ok(resolution)
    }

    pub fn allows_loading(&self) -> bool {
        self.decision == ProjectTrustDecision::Allow
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_TRUST_RESOLUTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROJECT_TRUST_POLICY_VERSION)
        {
            return Err("project_trust_resolution_header_invalid".to_owned());
        }
        valid_digest(&self.project_root_digest, "project_trust_project_digest")?;
        if self.candidates.len() > MAX_PROJECT_TRUST_ROOTS
            || self
                .candidates
                .iter()
                .any(|root| root.scope != self.requested_scope)
        {
            return Err("project_trust_resolution_scope_invalid".to_owned());
        }
        for root in &self.candidates {
            root.validate()?;
        }
        nonempty(&self.reason, "project_trust_reason", 128)?;
        if self.decision == ProjectTrustDecision::Allow
            && (self.selected_scope.is_none() || self.selected_root_digest.is_none())
        {
            return Err("project_trust_allow_selection_missing".to_owned());
        }
        if self.decision == ProjectTrustDecision::Deny && self.reason == "project_trust_allowed" {
            return Err("project_trust_deny_reason_invalid".to_owned());
        }
        if let Some(root_digest) = &self.selected_root_digest {
            valid_digest(root_digest, "project_trust_selected_digest")?;
            if !self
                .candidates
                .iter()
                .any(|root| &root.root_digest == root_digest)
            {
                return Err("project_trust_selected_root_missing".to_owned());
            }
        }
        valid_digest(&self.resolution_digest, "project_trust_resolution_digest")?;
        if self.resolution_digest != self.digest() {
            return Err("project_trust_resolution_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "project_root_digest": self.project_root_digest,
            "requested_scope": self.requested_scope,
            "candidates": self.candidates,
            "selected_scope": self.selected_scope,
            "selected_root_digest": self.selected_root_digest,
            "decision": self.decision,
            "reason": self.reason,
        }))
    }
}
