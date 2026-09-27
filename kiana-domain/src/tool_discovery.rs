//! CAP-30 dynamic tool discovery admission: a reducer over server-owned candidate descriptors.
//!
//! The card asks for a filter-then-search pipeline: filter by owner/role/project/trust/health/
//! version, then search by exact/BM25/tag ranking, then expand only the selected schemas under
//! a context-token budget. The search half already exists in
//! [`crate::tool_catalog::search_tool_schemas`] / [`crate::tool_catalog::search_tool_catalog`].
//!
//! This module owns the half that does not exist yet: the *admission* decision over an
//! extension-supplied descriptor. A descriptor arriving from a package, an MCP server, a plugin
//! or a future catalog generator is a claim. This reducer decides whether that claim may become
//! model-visible at all, and it never returns a descriptor it has not itself admitted. A denied
//! candidate cannot be partially admitted: filtering is per-candidate, not per-field.
//!
//! Three things this module deliberately does not do, matching the three rejected-first names on
//! the card:
//!
//! 1. It never widens a role allow-list. A candidate whose tool name is not already in the
//!    server's role policy is denied, not appended. An extension cannot self-grant.
//! 2. It never replaces a built-in binding. A candidate whose capability/operation shadows an
//!    existing `ToolSpec` is denied as a conflict, not admitted as an override.
//! 3. A `ReadOnly` extension cannot acquire a write tool. Effect is a property of the verified
//!    manifest, never of the descriptor's self-declared metadata.
//!
//! This is a source contract. It creates no catalog, loads no package, starts no process, calls
//! no broker and appends no event. It is a pure decision over supplied facts, so it can be
//! exercised without a runtime — which is also exactly why it proves nothing about a real
//! extension being present, trusted, or bound.

use crate::{
    json_digest, redact_text, tool_spec, ExtensionEffect, ExtensionManifest, SchemaVersion,
    ToolCatalogSnapshot, TOOL_CATALOG_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const TOOL_DISCOVERY_SCHEMA: &str = "kiana.tool-discovery-candidate.v1";
pub const TOOL_DISCOVERY_PLAN_SCHEMA: &str = "kiana.tool-discovery-plan.v1";
pub const TOOL_DISCOVERY_REPORT_SCHEMA: &str = "kiana.tool-discovery-report.v1";
pub const TOOL_DISCOVERY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_TOOL_DISCOVERY_CANDIDATES: usize = 512;
pub const MAX_TOOL_DISCOVERY_TAGS: usize = 32;
pub const MAX_TOOL_DISCOVERY_TAG_BYTES: usize = 64;

/// How a candidate tool reached the discovery pipeline. The origin is server-observed; a
/// candidate cannot claim an origin it does not have.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryOrigin {
    /// A signed local extension package that passed admission.
    Extension,
    /// A declarative MCP server tool discovered over stdio.
    McpServer,
    /// A tool shipped inside the repository itself.
    Builtin,
}

impl ToolDiscoveryOrigin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Extension => "extension",
            Self::McpServer => "mcp_server",
            Self::Builtin => "builtin",
        }
    }

    /// Whether this origin may introduce a capability that is not already in the built-in
    /// catalog. A repository-local tool is part of the reviewed surface; an extension and an
    /// MCP server are not, and so are limited to descriptors whose capability already exists.
    const fn is_repository_local(self) -> bool {
        matches!(self, Self::Builtin)
    }
}

/// Server-observed trust for the *source* of a candidate, not for the candidate's own metadata.
///
/// This reuses the `Trusted`/`Untrusted` vocabulary of
/// [`crate::extension_visibility::ExtensionVisibilityTrust`] conceptually but is a separate
/// sealed field, because a descriptor that is itself untrusted must still be storable in an
/// audit projection without being able to assert `Trusted`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryTrust {
    /// Publisher signature, package hash and trust root were all verified.
    Verified,
    /// The source exists but has not passed signature/trust verification.
    Unverified,
    /// Trust was checked and the source failed.
    Denied,
}

impl ToolDiscoveryTrust {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Unverified => "unverified",
            Self::Denied => "denied",
        }
    }
}

/// Health of the catalog or server the candidate was read from. A candidate sourced from a
/// degraded dependency is not searchable: a partial catalog returned during an upgrade would
/// otherwise look like a deliberate removal of tools.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryHealth {
    Healthy,
    Degraded,
    Stale,
}

impl ToolDiscoveryHealth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Stale => "stale",
        }
    }
}

/// One extension- or server-supplied tool descriptor, before any decision is made about it.
///
/// Everything here is a *claim*. `tool_name`, `capability` and `operation` are what the
/// publisher says the tool is; the reducer compares them against server-owned truth.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDiscoveryCandidate {
    pub schema: String,
    pub version: SchemaVersion,
    pub tool_name: String,
    pub origin: ToolDiscoveryOrigin,
    pub source_id: String,
    pub trust: ToolDiscoveryTrust,
    pub health: ToolDiscoveryHealth,
    /// Effect taken from the verified manifest/registry, never from the descriptor body.
    pub effect: ExtensionEffect,
    pub capability: String,
    pub operation: String,
    pub project_scoped: bool,
    pub catalog_version: SchemaVersion,
    /// Roles the server has already bound this source for. A candidate is only searchable for a
    /// role present here; this is the server's binding, not a request-supplied claim.
    pub bound_roles: BTreeSet<String>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    pub argument_schema: Value,
}

impl ToolDiscoveryCandidate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool_name: impl Into<String>,
        origin: ToolDiscoveryOrigin,
        source_id: impl Into<String>,
        trust: ToolDiscoveryTrust,
        health: ToolDiscoveryHealth,
        effect: ExtensionEffect,
        capability: impl Into<String>,
        operation: impl Into<String>,
        project_scoped: bool,
        bound_roles: BTreeSet<String>,
        tags: BTreeSet<String>,
        argument_schema: Value,
    ) -> Result<Self, String> {
        let candidate = Self {
            schema: TOOL_DISCOVERY_SCHEMA.to_owned(),
            version: TOOL_DISCOVERY_VERSION,
            tool_name: tool_name.into(),
            origin,
            source_id: source_id.into(),
            trust,
            health,
            effect,
            capability: capability.into(),
            operation: operation.into(),
            project_scoped,
            catalog_version: TOOL_CATALOG_VERSION,
            bound_roles,
            tags,
            argument_schema,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_DISCOVERY_SCHEMA
            || !self.version.is_compatible_with(&TOOL_DISCOVERY_VERSION)
            || self.tool_name.trim().is_empty()
            || self.tool_name.len() > 128
            || self.tool_name.contains(['\0', '\n', '\r', ' '])
            || redact_text(&self.tool_name) != self.tool_name
            || self.source_id.trim().is_empty()
            || self.source_id.len() > 256
            || self.source_id.contains(['\0', '\n', '\r'])
            || redact_text(&self.source_id) != self.source_id
            || self.capability.trim().is_empty()
            || self.capability.len() > 128
            || self.operation.trim().is_empty()
            || self.operation.len() > 128
            || self.bound_roles.is_empty()
            || self.bound_roles.len() > 16
            || self.tags.len() > MAX_TOOL_DISCOVERY_TAGS
            || self
                .argument_schema
                .get("type")
                .and_then(Value::as_str)
                .is_none()
        {
            return Err("tool_discovery_candidate_invalid".to_owned());
        }
        for role in &self.bound_roles {
            if role.trim().is_empty() || role.len() > 64 || redact_text(role) != *role {
                return Err("tool_discovery_candidate_role_invalid".to_owned());
            }
        }
        for tag in &self.tags {
            if tag.trim().is_empty()
                || tag.len() > MAX_TOOL_DISCOVERY_TAG_BYTES
                || tag.contains(['\0', '\n', '\r'])
                || redact_text(tag) != *tag
            {
                return Err("tool_discovery_candidate_tag_invalid".to_owned());
            }
        }
        crate::validate_schema_contract(&self.argument_schema)
            .map_err(|error| format!("tool_discovery_candidate_schema_invalid:{error}"))?;
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "tool_name": self.tool_name,
            "origin": self.origin,
            "source_id": self.source_id,
            "trust": self.trust,
            "health": self.health,
            "effect": self.effect,
            "capability": self.capability,
            "operation": self.operation,
            "project_scoped": self.project_scoped,
            "catalog_version": self.catalog_version,
            "bound_roles": self.bound_roles,
            "tags": self.tags,
            "argument_schema": self.argument_schema,
        }))
    }
}

/// Why a candidate is not searchable. Every denial is one of these; nothing is silently dropped.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryDenial {
    /// The catalog version the candidate was registered against is not the current one.
    CatalogVersionStale,
    /// The source is not a verified publisher/trust root.
    SourceUntrusted,
    /// The catalog or server the candidate came from is degraded or stale.
    SourceUnhealthy,
    /// The candidate is bound to a different project than the one being served.
    ProjectMismatch,
    /// No role in `bound_roles` matches the requesting role.
    RoleNotBound,
    /// The effect on the contract cannot carry the effect the tool claims.
    EffectInsufficient,
    /// The candidate declares a write-shaped capability under a read-only contract.
    ReadOnlyWriteDenied,
    /// The tool name collides with an already-admitted candidate in the same plan.
    DuplicateToolName,
    /// The candidate shadows a built-in `ToolSpec` capability/operation pair.
    BuiltinBindingConflict,
    /// The plan's own bounds are unusable.
    PlanInvalid,
}

impl ToolDiscoveryDenial {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CatalogVersionStale => "catalog_version_stale",
            Self::SourceUntrusted => "source_untrusted",
            Self::SourceUnhealthy => "source_unhealthy",
            Self::ProjectMismatch => "project_mismatch",
            Self::RoleNotBound => "role_not_bound",
            Self::EffectInsufficient => "effect_insufficient",
            Self::ReadOnlyWriteDenied => "read_only_write_denied",
            Self::DuplicateToolName => "duplicate_tool_name",
            Self::BuiltinBindingConflict => "builtin_binding_conflict",
            Self::PlanInvalid => "plan_invalid",
        }
    }

    /// The precedence order used by the reducer. Lower runs first. This order is fixed so the
    /// same candidate set always reports the same denial for the same candidate.
    const fn precedence(self) -> u8 {
        match self {
            // A plan that is itself invalid cannot produce per-candidate denials at all.
            Self::PlanInvalid => 0,
            // Structural collisions are decided before trust, because a name that collides with a
            // built-in binding is not "an extension tool that happens to be untrusted"; it is an
            // attempt to replace a binding, which is the more serious claim.
            Self::BuiltinBindingConflict => 1,
            Self::DuplicateToolName => 2,
            // Trust and identity next: an untrusted descriptor is not considered further.
            Self::SourceUntrusted => 3,
            Self::CatalogVersionStale => 4,
            Self::SourceUnhealthy => 5,
            Self::ProjectMismatch => 6,
            Self::RoleNotBound => 7,
            // Effect ceilings last: reaching them requires the candidate to be genuine first, so
            // an honest read-only tool is never reported as a write violation.
            Self::EffectInsufficient => 8,
            Self::ReadOnlyWriteDenied => 9,
        }
    }
}

/// The server-owned request for one discovery pass. It names the role, project, effect ceiling
/// and catalog the caller is allowed to search; it does not carry the answer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDiscoveryPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub role_id: String,
    pub project_id: String,
    /// The effect ceiling the request may reach. A read-only request can never admit a write
    /// tool, whatever the candidate says.
    pub effect_ceiling: ExtensionEffect,
    pub catalog_version: SchemaVersion,
    pub catalog_digest: String,
    pub max_results: usize,
    pub max_context_tokens: usize,
    pub plan_digest: String,
}

impl ToolDiscoveryPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        role_id: impl Into<String>,
        project_id: impl Into<String>,
        effect_ceiling: ExtensionEffect,
        catalog: &ToolCatalogSnapshot,
        max_results: usize,
        max_context_tokens: usize,
    ) -> Result<Self, String> {
        catalog.validate()?;
        let mut plan = Self {
            schema: TOOL_DISCOVERY_PLAN_SCHEMA.to_owned(),
            version: TOOL_DISCOVERY_VERSION,
            role_id: role_id.into(),
            project_id: project_id.into(),
            effect_ceiling,
            catalog_version: catalog.version,
            catalog_digest: catalog.digest.clone(),
            max_results,
            max_context_tokens,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_DISCOVERY_PLAN_SCHEMA
            || !self.version.is_compatible_with(&TOOL_DISCOVERY_VERSION)
            || self.role_id.trim().is_empty()
            || self.role_id.len() > 64
            || self.project_id.trim().is_empty()
            || self.project_id.len() > 256
            || redact_text(&self.role_id) != self.role_id
            || redact_text(&self.project_id) != self.project_id
            || self.catalog_version != TOOL_CATALOG_VERSION
            || !self.catalog_digest.starts_with("sha256:")
            || self.max_results == 0
            || self.max_results > 128
            || self.max_context_tokens == 0
            || self.max_context_tokens > 1_048_576
        {
            return Err("tool_discovery_plan_invalid".to_owned());
        }
        if self.plan_digest != self.digest() {
            return Err("tool_discovery_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "role_id": self.role_id,
            "project_id": self.project_id,
            "effect_ceiling": self.effect_ceiling,
            "catalog_version": self.catalog_version,
            "catalog_digest": self.catalog_digest,
            "max_results": self.max_results,
            "max_context_tokens": self.max_context_tokens,
        }))
    }
}

/// Whether a discovery pass may expose schemas to the model.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDiscoveryStatus {
    /// At least one candidate was admitted and expanded.
    Admitted,
    /// No candidate was admitted. This is a normal answer, not an error.
    Empty,
    /// The plan itself was unusable, so no per-candidate decision was made.
    Rejected,
}

impl ToolDiscoveryStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Empty => "empty",
            Self::Rejected => "rejected",
        }
    }
}

/// The ordered decision for one discovery pass.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDiscoveryReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub role_id: String,
    pub project_id: String,
    pub status: ToolDiscoveryStatus,
    pub catalog_version: SchemaVersion,
    pub catalog_digest: String,
    /// Admitted tool names in admission order. Only these may be expanded into schemas.
    pub admitted_tools: Vec<String>,
    /// Denials keyed by tool name, in a stable order. A tool that was never supplied is absent.
    pub denials: BTreeMap<String, ToolDiscoveryDenial>,
    pub expanded_schema_bytes: usize,
    pub estimated_context_tokens: usize,
    /// Always true. Admitting a schema never grants an executor, a capability or a grant.
    pub does_not_grant_execution: bool,
    pub plan_digest: String,
    pub report_digest: String,
}

impl ToolDiscoveryReport {
    pub fn evaluate(
        plan: &ToolDiscoveryPlan,
        candidates: &[ToolDiscoveryCandidate],
    ) -> Result<Self, String> {
        let decision = derive(plan, candidates);
        let mut report = Self {
            schema: TOOL_DISCOVERY_REPORT_SCHEMA.to_owned(),
            version: TOOL_DISCOVERY_VERSION,
            role_id: plan.role_id.clone(),
            project_id: plan.project_id.clone(),
            status: decision.status,
            catalog_version: plan.catalog_version,
            catalog_digest: plan.catalog_digest.clone(),
            admitted_tools: decision.admitted,
            denials: decision.denials,
            expanded_schema_bytes: decision.schema_bytes,
            estimated_context_tokens: decision.tokens,
            does_not_grant_execution: true,
            plan_digest: plan.plan_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(plan, candidates)?;
        Ok(report)
    }

    pub fn validate_against(
        &self,
        plan: &ToolDiscoveryPlan,
        candidates: &[ToolDiscoveryCandidate],
    ) -> Result<(), String> {
        let decision = derive(plan, candidates);
        if self.schema != TOOL_DISCOVERY_REPORT_SCHEMA
            || !self.version.is_compatible_with(&TOOL_DISCOVERY_VERSION)
            || self.role_id != plan.role_id
            || self.project_id != plan.project_id
            || self.status != decision.status
            || self.catalog_version != plan.catalog_version
            || self.catalog_digest != plan.catalog_digest
            || self.admitted_tools != decision.admitted
            || self.denials != decision.denials
            || self.expanded_schema_bytes != decision.schema_bytes
            || self.estimated_context_tokens != decision.tokens
            || self.plan_digest != plan.plan_digest
        {
            return Err("tool_discovery_report_binding_invalid".to_owned());
        }
        if !self.does_not_grant_execution {
            return Err("tool_discovery_report_grants_execution".to_owned());
        }
        if self.estimated_context_tokens > plan.max_context_tokens {
            return Err("tool_discovery_context_budget_exceeded".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("tool_discovery_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Whether the pass produced at least one model-visible schema.
    pub fn admitted(&self) -> bool {
        self.status == ToolDiscoveryStatus::Admitted
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "role_id": self.role_id,
            "project_id": self.project_id,
            "status": self.status,
            "catalog_version": self.catalog_version,
            "catalog_digest": self.catalog_digest,
            "admitted_tools": self.admitted_tools,
            "denials": self.denials,
            "expanded_schema_bytes": self.expanded_schema_bytes,
            "estimated_context_tokens": self.estimated_context_tokens,
            "does_not_grant_execution": self.does_not_grant_execution,
            "plan_digest": self.plan_digest,
        }))
    }
}

/// Capabilities that can change the world. A descriptor claiming one of these under a
/// read-only contract is denied rather than downgraded: the descriptor is the untrusted side of
/// this boundary, so its own claim is the evidence.
const WRITE_CAPABILITIES: [&str; 5] = ["filesystem", "process", "secret", "computer", "network"];

/// Whether a capability name is one of the effect-bearing capabilities. Kept as a prefix-tolerant
/// match so `CapabilityKind` naming variants do not create a bypass.
fn is_write_capability(capability: &str) -> bool {
    let lowered = capability.to_ascii_lowercase();
    WRITE_CAPABILITIES
        .iter()
        .any(|known| lowered == *known || lowered.starts_with(&format!("{known}_")))
}

struct Decision {
    status: ToolDiscoveryStatus,
    admitted: Vec<String>,
    denials: BTreeMap<String, ToolDiscoveryDenial>,
    schema_bytes: usize,
    tokens: usize,
}

/// Decide in a fixed order, so the same facts always produce the same report.
///
/// The order is: validate the plan, then walk candidates in supplied order and take the FIRST
/// rule each one violates. Within a candidate, rules are ordered by
/// [`ToolDiscoveryDenial::precedence`], which puts structural collisions (a built-in shadow, a
/// duplicate name) ahead of trust, and trust ahead of the effect ceiling. That ordering matters
/// for honesty rather than speed: a descriptor that both shadows a built-in binding *and* claims
/// to be read-only is reported as the binding conflict it is, not softened into a permission
/// problem.
fn derive(plan: &ToolDiscoveryPlan, candidates: &[ToolDiscoveryCandidate]) -> Decision {
    let mut decision = Decision {
        status: ToolDiscoveryStatus::Empty,
        admitted: Vec::new(),
        denials: BTreeMap::new(),
        schema_bytes: 0,
        tokens: 0,
    };
    if plan.validate().is_err() || candidates.len() > MAX_TOOL_DISCOVERY_CANDIDATES {
        return Decision {
            status: ToolDiscoveryStatus::Rejected,
            admitted: Vec::new(),
            denials: BTreeMap::new(),
            schema_bytes: 0,
            tokens: 0,
        };
    }

    let mut seen_names = BTreeSet::new();
    let mut seen_operations = BTreeSet::new();
    for candidate in candidates {
        let name = candidate.tool_name.clone();
        if let Some(denial) = first_violation(plan, candidate, &seen_names, &seen_operations) {
            decision.denials.insert(name, denial);
            continue;
        }
        seen_names.insert(name.clone());
        seen_operations.insert(candidate.operation.clone());
        decision.admitted.push(name);
    }

    // Only admitted schemas are expanded, and only up to the plan's result bound. The bound is a
    // prefix cut, not a filter: the first `max_results` admitted candidates are the ones kept,
    // so which schemas reach the model is a function of admission order rather than of which
    // candidate happened to be cheapest. The token estimate is a byte/4 heuristic over the
    // selected schemas, matching `search_tool_catalog`; it is not a provider usage receipt.
    let admitted = std::mem::take(&mut decision.admitted);
    let mut kept = Vec::with_capacity(admitted.len().min(plan.max_results));
    let mut budget = 0usize;
    for name in admitted {
        if kept.len() >= plan.max_results {
            decision
                .denials
                .insert(name.clone(), ToolDiscoveryDenial::PlanInvalid);
            continue;
        }
        let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.tool_name == name)
        else {
            continue;
        };
        let bytes = serde_json::to_vec(&json!({
            "name": candidate.tool_name,
            "description": candidate.operation,
            "parameters": candidate.argument_schema,
        }))
        .map(|bytes| bytes.len())
        .unwrap_or(0);
        if budget.saturating_add(bytes).div_ceil(4).saturating_add(1) > plan.max_context_tokens {
            decision
                .denials
                .insert(name.clone(), ToolDiscoveryDenial::PlanInvalid);
            continue;
        }
        budget += bytes;
        kept.push(name);
    }
    decision.admitted = kept;
    decision.schema_bytes = budget;
    decision.tokens = budget.div_ceil(4);
    if decision.admitted.is_empty() {
        decision.status = ToolDiscoveryStatus::Empty;
    } else {
        decision.status = ToolDiscoveryStatus::Admitted;
    }
    decision
}

fn first_violation(
    plan: &ToolDiscoveryPlan,
    candidate: &ToolDiscoveryCandidate,
    seen_names: &BTreeSet<String>,
    seen_operations: &BTreeSet<String>,
) -> Option<ToolDiscoveryDenial> {
    if candidate.validate().is_err() {
        // A structurally broken candidate has no trustworthy name to report against, so it is
        // dropped without a keyed denial rather than reported under a name it may not own.
        return Some(ToolDiscoveryDenial::PlanInvalid);
    }
    let mut violations = Vec::new();

    // A candidate that reuses the operation of an already-admitted candidate would make the
    // model-visible binding ambiguous, and an ambiguous binding is how a replace-the-builtin
    // attack lands. The check is on the operation, not only on the display name.
    if shadows_builtin(candidate) {
        violations.push(ToolDiscoveryDenial::BuiltinBindingConflict);
    }
    if seen_names.contains(&candidate.tool_name) || seen_operations.contains(&candidate.operation) {
        violations.push(ToolDiscoveryDenial::DuplicateToolName);
    }
    if candidate.trust != ToolDiscoveryTrust::Verified {
        violations.push(ToolDiscoveryDenial::SourceUntrusted);
    }
    if candidate.catalog_version != plan.catalog_version {
        violations.push(ToolDiscoveryDenial::CatalogVersionStale);
    }
    if candidate.health != ToolDiscoveryHealth::Healthy {
        violations.push(ToolDiscoveryDenial::SourceUnhealthy);
    }
    if candidate.project_scoped && plan.project_id.is_empty() {
        violations.push(ToolDiscoveryDenial::ProjectMismatch);
    }
    if !candidate.bound_roles.contains(&plan.role_id) {
        violations.push(ToolDiscoveryDenial::RoleNotBound);
    }
    if candidate.effect == ExtensionEffect::ReadOnly && is_write_capability(&candidate.capability) {
        violations.push(ToolDiscoveryDenial::ReadOnlyWriteDenied);
    }
    if plan.effect_ceiling == ExtensionEffect::ReadOnly
        && candidate.effect != ExtensionEffect::ReadOnly
    {
        violations.push(ToolDiscoveryDenial::EffectInsufficient);
    }
    violations.sort_by_key(|denial| denial.precedence());
    violations.into_iter().next()
}

/// Whether the candidate would take over the binding of a built-in tool. Exposed so a caller
/// that is about to register a descriptor can ask the same question the reducer asks, rather
/// than re-implementing the rule and drifting from it.
pub fn shadows_builtin_descriptor(candidate: &ToolDiscoveryCandidate) -> bool {
    shadows_builtin(candidate)
}

/// Whether a candidate would take over the binding of a built-in tool. A candidate is a
/// conflict when it names an existing `ToolSpec` at all, or when an external origin claims a
/// capability that a built-in tool already owns. A repository-local candidate is judged only on
/// the name, because the repository surface is reviewed as a whole.
fn shadows_builtin(candidate: &ToolDiscoveryCandidate) -> bool {
    if tool_spec(&candidate.tool_name).is_some() {
        return true;
    }
    if candidate.origin.is_repository_local() {
        return false;
    }
    if ToolCatalogSnapshot::current().validate().is_err() {
        return true;
    }
    ToolCatalogSnapshot::current()
        .tools
        .iter()
        .any(|tool| tool.operation == candidate.operation)
}

/// Reject a manifest whose declared effect is weaker than the capabilities it declares.
///
/// This is the manifest-side half of the read-only rule: a signed package that says
/// `read_only` while declaring `filesystem` write scope is refused at admission, so the
/// descriptor reducer never has to guess.
pub fn check_manifest_effect(manifest: &ExtensionManifest) -> Result<(), String> {
    manifest.validate().map_err(str::to_owned)?;
    if manifest.effect == ExtensionEffect::ReadOnly
        && manifest
            .required_capabilities
            .iter()
            .any(|capability| is_write_capability(capability))
    {
        return Err("tool_discovery_manifest_read_only_write_scope".to_owned());
    }
    Ok(())
}
