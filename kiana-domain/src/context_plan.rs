//! Deterministic, explainable context compilation for one immutable harness step.
//!
//! Context text is classified as Product or Context. Product sections must originate from a
//! prompt source; files, Memory and tool output can only enter the lower-trust Context layer.
//! Compilation records inclusion/omission reasons and `ResolvedStepContext` freezes the route,
//! catalog, workspace and data epochs used for both estimation and provider submission.

use crate::{
    json_digest, EvidenceStatus, PromptAuthority, PromptBundle, SourceKind, SourceRef,
    StepIdentity, TokenBudget,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTEXT_PLAN_SCHEMA: &str = "kiana.context-plan.v1";
pub const RESOLVED_STEP_CONTEXT_SCHEMA: &str = "kiana.resolved-step-context.v1";
pub const CONTEXT_PLAN_MAX_ITEMS: usize = 256;
pub const CONTEXT_PLAN_MAX_TEXT_BYTES: usize = 512 * 1024;

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
        if self.authority == PromptAuthority::Product && self.source.kind != SourceKind::Prompt {
            return Err("context_product_source_untrusted".to_owned());
        }
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
                left.priority,
                &left.name,
            )
                .cmp(&(
                    right.authority != PromptAuthority::Product,
                    right.priority,
                    &right.name,
                ))
                .then_with(|| left.source.digest().cmp(&right.source.digest()))
        });
        let mut used = 0u64;
        for item in &mut items {
            let mandatory = item.authority == PromptAuthority::Product;
            let next = used.saturating_add(item.estimated_tokens);
            if mandatory {
                if next > token_limit {
                    return Err("context_plan_product_budget_exceeded".to_owned());
                }
                item.included = true;
                used = next;
            } else if next <= token_limit {
                item.included = true;
                used = next;
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
        let mut names = BTreeSet::new();
        let mut estimated = 0u64;
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
            }
        }
        if estimated != self.estimated_tokens || estimated > self.token_limit {
            return Err("context_plan_budget_drift".to_owned());
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
    pub plan_digest: String,
    pub model_profile: String,
    pub route_digest: String,
    pub catalog_digest: String,
    pub workspace_revision: u64,
    pub data_epoch: u64,
    pub rendered_prompt_digest: String,
    pub budget: TokenBudget,
    pub request_digest: String,
}

impl ResolvedStepContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        plan: &ContextPlan,
        step_identity: StepIdentity,
        model_profile: impl Into<String>,
        route_digest: impl Into<String>,
        catalog_digest: impl Into<String>,
        workspace_revision: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        plan.validate()?;
        step_identity.validate()?;
        let model_profile = model_profile.into();
        let route_digest = route_digest.into();
        let catalog_digest = catalog_digest.into();
        required(&model_profile, "resolved_context_model_profile", 256)?;
        digest(&route_digest, "resolved_context_route_digest")?;
        digest(&catalog_digest, "resolved_context_catalog_digest")?;
        if workspace_revision == 0 || data_epoch == 0 {
            return Err("resolved_context_revision_invalid".to_owned());
        }
        let rendered_prompt_digest = json_digest(&serde_json::json!({
            "prompt": plan.rendered_prompt()
        }));
        let budget = TokenBudget::new(plan.rendered_prompt().len(), 0, 0, 0, plan.token_limit);
        budget.validate().map_err(str::to_owned)?;
        let request_digest = json_digest(&serde_json::json!({
            "step_identity": step_identity,
            "plan_digest": plan.plan_digest,
            "model_profile": model_profile,
            "route_digest": route_digest,
            "catalog_digest": catalog_digest,
            "workspace_revision": workspace_revision,
            "data_epoch": data_epoch,
            "rendered_prompt_digest": rendered_prompt_digest,
            "budget": budget,
        }));
        let context = Self {
            schema: RESOLVED_STEP_CONTEXT_SCHEMA.to_owned(),
            step_identity,
            plan_digest: plan.plan_digest.clone(),
            model_profile,
            route_digest,
            catalog_digest,
            workspace_revision,
            data_epoch,
            rendered_prompt_digest,
            budget,
            request_digest,
        };
        context.validate_against(plan)?;
        Ok(context)
    }

    pub fn validate_against(&self, plan: &ContextPlan) -> Result<(), String> {
        plan.validate()?;
        self.step_identity.validate()?;
        if self.schema != RESOLVED_STEP_CONTEXT_SCHEMA || self.plan_digest != plan.plan_digest {
            return Err("resolved_context_plan_changed".to_owned());
        }
        required(&self.model_profile, "resolved_context_model_profile", 256)?;
        digest(&self.route_digest, "resolved_context_route_digest")?;
        digest(&self.catalog_digest, "resolved_context_catalog_digest")?;
        if self.workspace_revision == 0 || self.data_epoch == 0 {
            return Err("resolved_context_revision_invalid".to_owned());
        }
        if self.rendered_prompt_digest
            != json_digest(&serde_json::json!({"prompt":plan.rendered_prompt()}))
        {
            return Err("resolved_context_prompt_changed".to_owned());
        }
        self.budget.validate().map_err(str::to_owned)?;
        let expected = json_digest(&serde_json::json!({
            "step_identity": self.step_identity,
            "plan_digest": self.plan_digest,
            "model_profile": self.model_profile,
            "route_digest": self.route_digest,
            "catalog_digest": self.catalog_digest,
            "workspace_revision": self.workspace_revision,
            "data_epoch": self.data_epoch,
            "rendered_prompt_digest": self.rendered_prompt_digest,
            "budget": self.budget,
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
        workspace_revision: u64,
        data_epoch: u64,
    ) -> Result<(), String> {
        if self.route_digest != route_digest {
            return Err("resolved_context_route_changed".to_owned());
        }
        if self.catalog_digest != catalog_digest {
            return Err("resolved_context_catalog_changed".to_owned());
        }
        if self.workspace_revision != workspace_revision {
            return Err("resolved_context_workspace_changed".to_owned());
        }
        if self.data_epoch != data_epoch {
            return Err("resolved_context_data_epoch_changed".to_owned());
        }
        Ok(())
    }
}
