//! Deterministic, explainable context compilation for one immutable harness step.
//!
//! Context text is classified as Product or Context. Product sections must originate from a
//! prompt source; files, Memory and tool output can only enter the lower-trust Context layer.
//! Compilation records inclusion/omission reasons and `ResolvedStepContext` freezes the route,
//! catalog, workspace and data epochs used for both estimation and provider submission.

use crate::{
    json_digest, EvidenceStatus, PromptAuthority, PromptBundle, ScopeSet, SourceKind, SourceRef,
    SourceSnapshot, StepIdentity, TokenBudget, WireBudget,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTEXT_PLAN_SCHEMA: &str = "kiana.context-plan.v1";
pub const RESOLVED_STEP_CONTEXT_SCHEMA: &str = "kiana.resolved-step-context.v1";
pub const PREPARED_MODEL_REQUEST_SCHEMA: &str = "kiana.prepared-model-request.v1";
pub const CONTEXT_PLAN_MAX_ITEMS: usize = 256;
pub const CONTEXT_PLAN_MAX_TEXT_BYTES: usize = 512 * 1024;

/// The material classes which may enter one prepared context plan.
///
/// The order is part of the contract: lower values are selected first. Product material is
/// deliberately separate from all retrieved or user/project supplied context so a source can
/// never gain product authority by changing its label or relevance score.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMaterialType {
    ProductSystem,
    Role,
    Task,
    Packet,
    WorkspaceSnapshot,
    History,
    Memory,
    RepoMap,
    LiveResult,
}

impl ContextMaterialType {
    pub const ALL: [Self; 9] = [
        Self::ProductSystem,
        Self::Role,
        Self::Task,
        Self::Packet,
        Self::WorkspaceSnapshot,
        Self::History,
        Self::Memory,
        Self::RepoMap,
        Self::LiveResult,
    ];

    /// Stable selection order. This is not a user-controlled priority.
    pub const fn selection_priority(self) -> u8 {
        match self {
            Self::ProductSystem => 0,
            Self::Role => 1,
            Self::Task => 2,
            Self::Packet => 3,
            Self::WorkspaceSnapshot => 4,
            Self::History => 5,
            Self::Memory => 6,
            Self::RepoMap => 7,
            Self::LiveResult => 8,
        }
    }

    /// Share of the non-product source budget, in thousandths of the plan limit.
    const fn budget_milli(self) -> u64 {
        match self {
            Self::ProductSystem | Self::Role => 1_000,
            Self::Task => 300,
            Self::Packet => 250,
            Self::WorkspaceSnapshot => 250,
            Self::History => 200,
            Self::Memory => 200,
            Self::RepoMap => 200,
            Self::LiveResult => 150,
        }
    }

    fn budget_limit(self, token_limit: u64) -> u64 {
        if self.budget_milli() == 1_000 {
            token_limit
        } else {
            let scaled = token_limit.saturating_mul(self.budget_milli());
            (scaled / 1_000 + u64::from(scaled % 1_000 != 0)).max(1)
        }
    }

    fn index(self) -> usize {
        usize::from(self.selection_priority())
    }

    fn source_allowed(self, source: SourceKind) -> bool {
        match self {
            Self::ProductSystem | Self::Role => source == SourceKind::Prompt,
            Self::Task => matches!(
                source,
                SourceKind::Prompt | SourceKind::Event | SourceKind::UserImport
            ),
            Self::Packet => matches!(source, SourceKind::Artifact | SourceKind::Event),
            Self::WorkspaceSnapshot | Self::RepoMap => source == SourceKind::WorkspaceFile,
            Self::History => matches!(source, SourceKind::Event | SourceKind::Artifact),
            Self::Memory => source == SourceKind::Memory,
            Self::LiveResult => matches!(
                source,
                SourceKind::Connector
                    | SourceKind::Event
                    | SourceKind::Artifact
                    | SourceKind::WorkspaceFile
            ),
        }
    }

    fn validate_authority(
        self,
        authority: PromptAuthority,
        source: &SourceRef,
    ) -> Result<(), String> {
        let product = matches!(self, Self::ProductSystem | Self::Role);
        if product && authority != PromptAuthority::Product {
            return Err("context_product_authority_missing".to_owned());
        }
        if !product && authority == PromptAuthority::Product {
            return Err("context_product_material_type_invalid".to_owned());
        }
        if product {
            if source.kind != SourceKind::Prompt {
                return Err("context_product_source_untrusted".to_owned());
            }
            if source.evidence != EvidenceStatus::Verified {
                return Err("context_product_evidence_unverified".to_owned());
            }
        } else if !self.source_allowed(source.kind) {
            return Err("context_material_source_mismatch".to_owned());
        }
        Ok(())
    }

    fn from_prompt_section(section: &crate::PromptSection) -> Self {
        if section.authority == PromptAuthority::Product {
            if section.name == "role" {
                Self::Role
            } else {
                Self::ProductSystem
            }
        } else {
            Self::Task
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSourceBudget {
    pub material_type: ContextMaterialType,
    pub token_limit: u64,
    pub used_tokens: u64,
}

impl ContextSourceBudget {
    fn new(material_type: ContextMaterialType, plan_limit: u64) -> Self {
        Self {
            material_type,
            token_limit: material_type.budget_limit(plan_limit),
            used_tokens: 0,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.token_limit == 0 || self.used_tokens > self.token_limit {
            return Err("context_source_budget_invalid".to_owned());
        }
        Ok(())
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
pub struct ContextCandidate {
    pub name: String,
    pub text: String,
    pub source: SourceRef,
    pub authority: PromptAuthority,
    pub material_type: ContextMaterialType,
    pub permission_scope: String,
    pub revision: String,
    pub priority: u32,
}

impl ContextCandidate {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.name, "context_candidate_name", 256)?;
        if self.text.len() > CONTEXT_PLAN_MAX_TEXT_BYTES {
            return Err("context_candidate_text_too_large".to_owned());
        }
        self.source.validate()?;
        required(&self.permission_scope, "context_candidate_scope", 256)?;
        required(&self.revision, "context_candidate_revision", 256)?;
        self.material_type
            .validate_authority(self.authority, &self.source)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPlanItem {
    pub name: String,
    pub text: String,
    pub source: SourceRef,
    pub authority: PromptAuthority,
    pub material_type: ContextMaterialType,
    pub permission_scope: String,
    pub revision: String,
    pub priority: u32,
    pub estimated_tokens: u64,
    pub included: bool,
    pub omission_reason: Option<String>,
}

impl ContextPlanItem {
    fn from_candidate(candidate: ContextCandidate) -> Result<Self, String> {
        candidate.validate()?;
        Ok(Self {
            estimated_tokens: candidate.text.len() as u64,
            name: candidate.name,
            text: candidate.text,
            source: candidate.source,
            authority: candidate.authority,
            material_type: candidate.material_type,
            permission_scope: candidate.permission_scope,
            revision: candidate.revision,
            priority: candidate.priority,
            included: false,
            omission_reason: None,
        })
    }

    fn validate(&self) -> Result<(), String> {
        ContextCandidate {
            name: self.name.clone(),
            text: self.text.clone(),
            source: self.source.clone(),
            authority: self.authority,
            material_type: self.material_type,
            permission_scope: self.permission_scope.clone(),
            revision: self.revision.clone(),
            priority: self.priority,
        }
        .validate()?;
        if self.estimated_tokens != self.text.len() as u64 {
            return Err("context_plan_estimate_drift".to_owned());
        }
        if self.included && self.omission_reason.is_some() {
            return Err("context_plan_included_omission_conflict".to_owned());
        }
        if !self.included && self.omission_reason.is_none() {
            return Err("context_plan_omission_reason_missing".to_owned());
        }
        if let Some(reason) = self.omission_reason.as_deref() {
            if !matches!(
                reason,
                "context_source_budget_exceeded" | "context_budget_exceeded"
            ) {
                return Err("context_plan_omission_reason_invalid".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPlan {
    pub schema: String,
    pub role_id: String,
    pub prompt_bundle_digest: String,
    pub items: Vec<ContextPlanItem>,
    pub source_budgets: Vec<ContextSourceBudget>,
    pub token_limit: u64,
    pub estimated_tokens: u64,
    pub plan_digest: String,
}

impl ContextPlan {
    pub fn compile(
        bundle: &PromptBundle,
        candidates: Vec<ContextCandidate>,
        token_limit: u64,
    ) -> Result<Self, String> {
        bundle.validate()?;
        if token_limit == 0 || candidates.len() + bundle.sections.len() > CONTEXT_PLAN_MAX_ITEMS {
            return Err("context_plan_limit_invalid".to_owned());
        }
        let mut inputs = Vec::with_capacity(bundle.sections.len() + candidates.len());
        for section in &bundle.sections {
            let source = SourceRef::from_prompt_section(section)?;
            inputs.push(ContextCandidate {
                name: section.name.clone(),
                text: section.text.clone(),
                source,
                authority: section.authority,
                material_type: ContextMaterialType::from_prompt_section(section),
                permission_scope: if section.authority == PromptAuthority::Product {
                    "product".to_owned()
                } else {
                    "prompt-context".to_owned()
                },
                revision: format!("prompt-order:{}", section.order),
                priority: section.order,
            });
        }
        inputs.extend(candidates);
        let mut items = inputs
            .into_iter()
            .map(ContextPlanItem::from_candidate)
            .collect::<Result<Vec<_>, _>>()?;
        items.sort_by(|left, right| {
            (
                left.authority != PromptAuthority::Product,
                left.material_type.selection_priority(),
                left.priority,
                &left.name,
            )
                .cmp(&(
                    right.authority != PromptAuthority::Product,
                    right.material_type.selection_priority(),
                    right.priority,
                    &right.name,
                ))
                .then_with(|| left.source.digest().cmp(&right.source.digest()))
        });
        let mut source_budgets = ContextMaterialType::ALL
            .into_iter()
            .map(|material_type| ContextSourceBudget::new(material_type, token_limit))
            .collect::<Vec<_>>();
        let mut used = 0u64;
        for item in &mut items {
            let mandatory = item.authority == PromptAuthority::Product;
            let next = used.saturating_add(item.estimated_tokens);
            let budget = &mut source_budgets[item.material_type.index()];
            let next_source = budget.used_tokens.saturating_add(item.estimated_tokens);
            if mandatory {
                if next > token_limit {
                    return Err("context_plan_product_budget_exceeded".to_owned());
                }
                item.included = true;
                used = next;
                budget.used_tokens = next_source;
            } else if next_source > budget.token_limit {
                item.included = false;
                item.omission_reason = Some("context_source_budget_exceeded".to_owned());
            } else if next <= token_limit {
                item.included = true;
                used = next;
                budget.used_tokens = next_source;
            } else {
                item.included = false;
                item.omission_reason = Some("context_budget_exceeded".to_owned());
            }
        }
        for item in &items {
            item.validate()?;
        }
        let prompt_bundle_digest = json_digest(&serde_json::to_value(bundle).unwrap_or_default());
        let mut plan = Self {
            schema: CONTEXT_PLAN_SCHEMA.to_owned(),
            role_id: bundle.role_id.clone(),
            prompt_bundle_digest,
            items,
            source_budgets,
            token_limit,
            estimated_tokens: used,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_PLAN_SCHEMA
            || self.items.is_empty()
            || self.items.len() > CONTEXT_PLAN_MAX_ITEMS
            || self.token_limit == 0
        {
            return Err("context_plan_header_invalid".to_owned());
        }
        required(&self.role_id, "context_plan_role", 128)?;
        digest(
            &self.prompt_bundle_digest,
            "context_plan_prompt_bundle_digest",
        )?;
        if self.source_budgets.len() != ContextMaterialType::ALL.len() {
            return Err("context_source_budget_set_invalid".to_owned());
        }
        for (index, budget) in self.source_budgets.iter().enumerate() {
            budget.validate()?;
            if budget.material_type != ContextMaterialType::ALL[index] {
                return Err("context_source_budget_order_invalid".to_owned());
            }
            if budget.token_limit != budget.material_type.budget_limit(self.token_limit) {
                return Err("context_source_budget_policy_drift".to_owned());
            }
        }
        let mut names = BTreeSet::new();
        let mut estimated = 0u64;
        let mut source_used = vec![0u64; ContextMaterialType::ALL.len()];
        if !self
            .items
            .iter()
            .any(|item| item.authority == PromptAuthority::Product && item.included)
        {
            return Err("context_plan_product_section_missing".to_owned());
        }
        for item in &self.items {
            item.validate()?;
            if !names.insert(item.name.clone()) {
                return Err("context_plan_duplicate_name".to_owned());
            }
            if item.included {
                estimated = estimated.saturating_add(item.estimated_tokens);
                source_used[item.material_type.index()] =
                    source_used[item.material_type.index()].saturating_add(item.estimated_tokens);
            }
        }
        if estimated != self.estimated_tokens || estimated > self.token_limit {
            return Err("context_plan_budget_drift".to_owned());
        }
        for (index, budget) in self.source_budgets.iter().enumerate() {
            if source_used[index] != budget.used_tokens {
                return Err("context_source_budget_drift".to_owned());
            }
        }
        digest(&self.plan_digest, "context_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("context_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn rendered_prompt(&self) -> String {
        self.items
            .iter()
            .filter(|item| item.included)
            .map(|item| item.text.as_str())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn omission_reasons(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .filter_map(|item| {
                item.omission_reason
                    .as_ref()
                    .map(|reason| (item.name.clone(), reason.clone()))
            })
            .collect()
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "plan_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedStepContext {
    pub schema: String,
    pub step_identity: StepIdentity,
    pub scope: ScopeSet,
    pub context_plan: ContextPlan,
    pub plan_digest: String,
    pub prompt_bundle_digest: String,
    pub model_profile: String,
    pub route_digest: String,
    pub catalog_digest: String,
    pub tool_catalog_digest: String,
    pub source_snapshots: Vec<SourceSnapshot>,
    pub workspace_revision: u64,
    pub data_epoch: u64,
    pub rendered_prompt_digest: String,
    pub budget: TokenBudget,
    pub wire_budget: WireBudget,
    pub request_digest: String,
}

/// The provider-facing name for the same immutable snapshot. Estimate, send and receipt code
/// must consume this value rather than compiling a second request from mutable inputs.
pub type PreparedModelRequest = ResolvedStepContext;

impl ResolvedStepContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        plan: &ContextPlan,
        scope: ScopeSet,
        step_identity: StepIdentity,
        model_profile: impl Into<String>,
        route_digest: impl Into<String>,
        catalog_digest: impl Into<String>,
        source_snapshots: Vec<SourceSnapshot>,
        wire_budget: WireBudget,
        workspace_revision: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        plan.validate()?;
        scope.validate()?;
        step_identity.validate()?;
        wire_budget.validate()?;
        let model_profile = model_profile.into();
        let route_digest = route_digest.into();
        let catalog_digest = catalog_digest.into();
        required(&model_profile, "resolved_context_model_profile", 256)?;
        digest(&route_digest, "resolved_context_route_digest")?;
        digest(&catalog_digest, "resolved_context_catalog_digest")?;
        if workspace_revision == 0 || data_epoch == 0 {
            return Err("resolved_context_revision_invalid".to_owned());
        }
        validate_source_snapshots(&source_snapshots)?;
        let rendered_prompt_digest = json_digest(&serde_json::json!({
            "prompt": plan.rendered_prompt()
        }));
        let budget = TokenBudget::new(plan.rendered_prompt().len(), 0, 0, 0, plan.token_limit);
        budget.validate().map_err(str::to_owned)?;
        let request_digest = json_digest(&serde_json::json!({
            "schema": PREPARED_MODEL_REQUEST_SCHEMA,
            "step_identity": step_identity,
            "scope": scope,
            "context_plan": plan,
            "plan_digest": plan.plan_digest,
            "prompt_bundle_digest": plan.prompt_bundle_digest,
            "model_profile": model_profile,
            "route_digest": route_digest,
            "catalog_digest": catalog_digest,
            "tool_catalog_digest": catalog_digest,
            "source_snapshots": source_snapshots,
            "workspace_revision": workspace_revision,
            "data_epoch": data_epoch,
            "rendered_prompt_digest": rendered_prompt_digest,
            "budget": budget,
            "wire_budget": wire_budget,
        }));
        let context = Self {
            schema: PREPARED_MODEL_REQUEST_SCHEMA.to_owned(),
            step_identity,
            scope,
            context_plan: plan.clone(),
            plan_digest: plan.plan_digest.clone(),
            prompt_bundle_digest: plan.prompt_bundle_digest.clone(),
            model_profile,
            route_digest,
            tool_catalog_digest: catalog_digest.clone(),
            catalog_digest,
            source_snapshots,
            workspace_revision,
            data_epoch,
            rendered_prompt_digest,
            budget,
            wire_budget,
            request_digest,
        };
        context.validate_against(plan)?;
        Ok(context)
    }

    pub fn validate_against(&self, plan: &ContextPlan) -> Result<(), String> {
        plan.validate()?;
        self.step_identity.validate()?;
        self.scope.validate()?;
        self.context_plan.validate()?;
        if (self.schema != RESOLVED_STEP_CONTEXT_SCHEMA
            && self.schema != PREPARED_MODEL_REQUEST_SCHEMA)
            || self.plan_digest != plan.plan_digest
        {
            return Err("resolved_context_plan_changed".to_owned());
        }
        if self.context_plan.plan_digest != plan.plan_digest
            || self.prompt_bundle_digest != plan.prompt_bundle_digest
            || self.context_plan.prompt_bundle_digest != self.prompt_bundle_digest
        {
            return Err("resolved_context_plan_changed".to_owned());
        }
        required(&self.model_profile, "resolved_context_model_profile", 256)?;
        digest(&self.route_digest, "resolved_context_route_digest")?;
        digest(&self.catalog_digest, "resolved_context_catalog_digest")?;
        if self.tool_catalog_digest != self.catalog_digest {
            return Err("resolved_context_catalog_changed".to_owned());
        }
        if self.workspace_revision == 0 || self.data_epoch == 0 {
            return Err("resolved_context_revision_invalid".to_owned());
        }
        validate_source_snapshots(&self.source_snapshots)?;
        if self.rendered_prompt_digest
            != json_digest(&serde_json::json!({"prompt":plan.rendered_prompt()}))
        {
            return Err("resolved_context_prompt_changed".to_owned());
        }
        self.budget.validate().map_err(str::to_owned)?;
        self.wire_budget.validate()?;
        let expected = json_digest(&serde_json::json!({
            "schema": self.schema,
            "step_identity": self.step_identity,
            "scope": self.scope,
            "context_plan": self.context_plan,
            "plan_digest": self.plan_digest,
            "prompt_bundle_digest": self.prompt_bundle_digest,
            "model_profile": self.model_profile,
            "route_digest": self.route_digest,
            "catalog_digest": self.catalog_digest,
            "tool_catalog_digest": self.tool_catalog_digest,
            "source_snapshots": self.source_snapshots,
            "workspace_revision": self.workspace_revision,
            "data_epoch": self.data_epoch,
            "rendered_prompt_digest": self.rendered_prompt_digest,
            "budget": self.budget,
            "wire_budget": self.wire_budget,
        }));
        if self.request_digest != expected {
            return Err("resolved_context_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn recheck_bindings(
        &self,
        route_digest: &str,
        catalog_digest: &str,
        scope_digest: &str,
        workspace_revision: u64,
        data_epoch: u64,
    ) -> Result<(), String> {
        if self.route_digest != route_digest {
            return Err("resolved_context_route_changed".to_owned());
        }
        if self.catalog_digest != catalog_digest {
            return Err("resolved_context_catalog_changed".to_owned());
        }
        if self.scope.scope_digest != scope_digest {
            return Err("resolved_context_scope_changed".to_owned());
        }
        if self.workspace_revision != workspace_revision {
            return Err("resolved_context_workspace_changed".to_owned());
        }
        if self.data_epoch != data_epoch {
            return Err("resolved_context_data_epoch_changed".to_owned());
        }
        Ok(())
    }

    pub fn rendered_prompt(&self) -> String {
        self.context_plan.rendered_prompt()
    }
}

fn validate_source_snapshots(source_snapshots: &[SourceSnapshot]) -> Result<(), String> {
    if source_snapshots.len() > CONTEXT_PLAN_MAX_ITEMS {
        return Err("resolved_context_source_snapshot_limit".to_owned());
    }
    let mut digests = BTreeSet::new();
    for snapshot in source_snapshots {
        snapshot.validate()?;
        if !digests.insert(snapshot.snapshot_digest.clone()) {
            return Err("resolved_context_source_snapshot_duplicate".to_owned());
        }
    }
    Ok(())
}
