//! Versioned CompanyOS process templates and configuration governance.
//!
//! A template version freezes roles, gates, required artifacts, output contract and the policy
//! profile it references.  The registry is a pure, append-only projection: installing an initial
//! template is allowed once, while every later change is an explicitly accepted proposal.  A
//! process pin is never rewritten by an upgrade or rollback; migration is a separate operation.
//! Prompt text, `allowed-tools`, untrusted packages and hidden scripts are configuration metadata,
//! never execution authority.

use crate::{json_digest, PolicyProfile, PolicyProfileStatus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_TEMPLATE_REGISTRY_SCHEMA: &str = "kiana.company-template-registry.v1";
pub const COMPANY_ROLE_PACK_SCHEMA: &str = "kiana.company-role-pack.v1";
pub const COMPANY_TEMPLATE_VERSION_SCHEMA: &str = "kiana.company-template-version.v1";
pub const COMPANY_TEMPLATE_PROPOSAL_SCHEMA: &str = "kiana.company-template-proposal.v1";
pub const COMPANY_TEMPLATE_ROLLBACK_SCHEMA: &str = "kiana.company-template-rollback.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

fn prompt_hash(value: &str, field: &'static str) -> Result<(), &'static str> {
    if let Some(hex) = value.strip_prefix("fnv1a64:") {
        if hex.len() == 16 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(());
        }
    }
    digest(value, field)
}

fn unique(values: &[String], field: &'static str, max: usize) -> Result<(), &'static str> {
    if values.is_empty() || values.len() > max {
        return Err(field);
    }
    let mut seen = BTreeSet::new();
    for value in values {
        required(value, field)?;
        if !seen.insert(value) {
            return Err(field);
        }
    }
    Ok(())
}

fn optional_ref(value: &Option<String>, field: &'static str) -> Result<(), &'static str> {
    if let Some(value) = value {
        required(value, field)?;
        if value.to_ascii_lowercase().contains("private") {
            return Err(field);
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyTemplateKind {
    Coding,
    ResearchReport,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyOutputContract {
    CodingSourceChange,
    ResearchReportLocalDelivery,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyTemplateStatus {
    Active,
    Retired,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyConfigSource {
    Builtin,
    TrustedOperator,
    Prompt,
    UntrustedPackage,
    HiddenScript,
}

impl CompanyConfigSource {
    fn validate_authority_boundary(self) -> Result<(), &'static str> {
        match self {
            Self::Builtin | Self::TrustedOperator => Ok(()),
            Self::Prompt => Err("company_template_prompt_not_authority"),
            Self::UntrustedPackage => Err("company_template_untrusted_package_not_authority"),
            Self::HiddenScript => Err("company_template_hidden_script_not_authority"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyConfigProposalStatus {
    Proposed,
    Approved,
    Rejected,
    RolledBack,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyRolePackRef {
    pub role_id: String,
    pub version: u64,
    pub digest: String,
}

impl CompanyRolePackRef {
    pub fn validate(&self) -> Result<(), &'static str> {
        required(&self.role_id, "company_role_pack_ref_role_required")?;
        if self.version == 0 {
            return Err("company_role_pack_ref_version_invalid");
        }
        digest(&self.digest, "company_role_pack_ref_digest_invalid")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyRolePackVersion {
    pub schema: String,
    pub role_id: String,
    pub version: u64,
    pub prompt_hash: String,
    pub input_schema: String,
    pub output_schema: String,
    pub model_profile: String,
    pub policy_profile_id: String,
    pub policy_profile_version: u64,
    pub policy_profile_digest: String,
    pub digest: String,
}

impl CompanyRolePackVersion {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_ROLE_PACK_SCHEMA || self.version == 0 {
            return Err("company_role_pack_header_invalid");
        }
        for (value, field) in [
            (&self.role_id, "company_role_pack_role_required"),
            (
                &self.input_schema,
                "company_role_pack_input_schema_required",
            ),
            (
                &self.output_schema,
                "company_role_pack_output_schema_required",
            ),
            (
                &self.model_profile,
                "company_role_pack_model_profile_required",
            ),
            (
                &self.policy_profile_id,
                "company_role_pack_policy_profile_required",
            ),
        ] {
            required(value, field)?;
        }
        if self.policy_profile_version == 0 {
            return Err("company_role_pack_policy_profile_version_invalid");
        }
        prompt_hash(&self.prompt_hash, "company_role_pack_prompt_hash_invalid")?;
        digest(
            &self.policy_profile_digest,
            "company_role_pack_policy_profile_digest_invalid",
        )?;
        digest(&self.digest, "company_role_pack_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_role_pack_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "role_id": self.role_id,
            "version": self.version,
            "prompt_hash": self.prompt_hash,
            "input_schema": self.input_schema,
            "output_schema": self.output_schema,
            "model_profile": self.model_profile,
            "policy_profile_id": self.policy_profile_id,
            "policy_profile_version": self.policy_profile_version,
            "policy_profile_digest": self.policy_profile_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyTemplateVersion {
    pub schema: String,
    pub template_id: String,
    pub version: u64,
    pub kind: CompanyTemplateKind,
    pub role_packs: Vec<CompanyRolePackRef>,
    pub role_order: Vec<String>,
    pub required_artifacts: Vec<String>,
    pub required_gates: Vec<String>,
    pub output_contract: CompanyOutputContract,
    pub policy_profile_id: String,
    pub policy_profile_version: u64,
    pub policy_profile_digest: String,
    pub status: CompanyTemplateStatus,
    pub template_hash: String,
}

impl CompanyTemplateVersion {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_TEMPLATE_VERSION_SCHEMA || self.version == 0 {
            return Err("company_template_header_invalid");
        }
        required(&self.template_id, "company_template_id_required")?;
        unique(&self.role_order, "company_template_role_order_invalid", 64)?;
        unique(
            &self.required_artifacts,
            "company_template_artifacts_invalid",
            128,
        )?;
        unique(&self.required_gates, "company_template_gates_invalid", 128)?;
        if self.role_packs.is_empty() || self.role_packs.len() > 64 {
            return Err("company_template_role_packs_invalid");
        }
        for role_pack in &self.role_packs {
            role_pack.validate()?;
        }
        if self
            .role_packs
            .iter()
            .map(|role| role.role_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != self.role_packs.len()
        {
            return Err("company_template_role_pack_duplicate");
        }
        if self.policy_profile_version == 0 {
            return Err("company_template_policy_profile_version_invalid");
        }
        required(
            &self.policy_profile_id,
            "company_template_policy_profile_required",
        )?;
        digest(
            &self.policy_profile_digest,
            "company_template_policy_profile_digest_invalid",
        )?;
        match self.kind {
            CompanyTemplateKind::Coding => {
                if self.output_contract != CompanyOutputContract::CodingSourceChange
                    || !self.role_order.iter().any(|role| role == "builder")
                {
                    return Err("company_coding_template_contract_invalid");
                }
            }
            CompanyTemplateKind::ResearchReport => {
                if self.output_contract != CompanyOutputContract::ResearchReportLocalDelivery
                    || self.role_order.iter().any(|role| role == "builder")
                    || !self.role_order.iter().any(|role| role == "analyst")
                    || !self.role_order.iter().any(|role| role == "reviewer")
                {
                    return Err("company_research_template_contract_invalid");
                }
            }
        }
        if self.template_hash != self.canonical_hash() {
            return Err("company_template_hash_mismatch");
        }
        Ok(())
    }

    pub fn canonical_hash(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "template_id": self.template_id,
            "version": self.version,
            "kind": self.kind,
            "role_packs": self.role_packs,
            "role_order": self.role_order,
            "required_artifacts": self.required_artifacts,
            "required_gates": self.required_gates,
            "output_contract": self.output_contract,
            "policy_profile_id": self.policy_profile_id,
            "policy_profile_version": self.policy_profile_version,
            "policy_profile_digest": self.policy_profile_digest,
        }))
    }

    pub fn coding_v1(
        template_id: impl Into<String>,
        role_packs: Vec<CompanyRolePackRef>,
        policy_profile_id: impl Into<String>,
        policy_profile_digest: impl Into<String>,
    ) -> Self {
        let mut template = Self {
            schema: COMPANY_TEMPLATE_VERSION_SCHEMA.to_owned(),
            template_id: template_id.into(),
            version: 1,
            kind: CompanyTemplateKind::Coding,
            role_packs,
            role_order: vec![
                "sponsor".to_owned(),
                "pm".to_owned(),
                "builder".to_owned(),
                "reviewer".to_owned(),
                "closer".to_owned(),
            ],
            required_artifacts: vec![
                "charter".to_owned(),
                "plan".to_owned(),
                "source_change".to_owned(),
                "independent_review".to_owned(),
                "delivery_manifest".to_owned(),
            ],
            required_gates: vec![
                "charter_acceptance".to_owned(),
                "independent_review".to_owned(),
                "delivery_confirmation".to_owned(),
                "closing_receipt".to_owned(),
            ],
            output_contract: CompanyOutputContract::CodingSourceChange,
            policy_profile_id: policy_profile_id.into(),
            policy_profile_version: 1,
            policy_profile_digest: policy_profile_digest.into(),
            status: CompanyTemplateStatus::Active,
            template_hash: String::new(),
        };
        template.template_hash = template.canonical_hash();
        template
    }

    pub fn research_report_v1(
        template_id: impl Into<String>,
        role_packs: Vec<CompanyRolePackRef>,
        policy_profile_id: impl Into<String>,
        policy_profile_digest: impl Into<String>,
    ) -> Self {
        let mut template = Self {
            schema: COMPANY_TEMPLATE_VERSION_SCHEMA.to_owned(),
            template_id: template_id.into(),
            version: 1,
            kind: CompanyTemplateKind::ResearchReport,
            role_packs,
            role_order: vec![
                "sponsor".to_owned(),
                "pm".to_owned(),
                "analyst".to_owned(),
                "reviewer".to_owned(),
                "closer".to_owned(),
            ],
            required_artifacts: vec![
                "charter".to_owned(),
                "research_question".to_owned(),
                "research_notes".to_owned(),
                "independent_check".to_owned(),
                "local_report".to_owned(),
            ],
            required_gates: vec![
                "charter_acceptance".to_owned(),
                "independent_check".to_owned(),
                "local_delivery_confirmation".to_owned(),
                "closing_receipt".to_owned(),
            ],
            output_contract: CompanyOutputContract::ResearchReportLocalDelivery,
            policy_profile_id: policy_profile_id.into(),
            policy_profile_version: 1,
            policy_profile_digest: policy_profile_digest.into(),
            status: CompanyTemplateStatus::Active,
            template_hash: String::new(),
        };
        template.template_hash = template.canonical_hash();
        template
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveTemplatePin {
    pub schema: String,
    pub process_id: String,
    pub template_id: String,
    pub template_version: u64,
    pub template_hash: String,
    pub policy_profile_id: String,
    pub policy_profile_version: u64,
    pub policy_profile_digest: String,
    pub authority_digest: String,
}

impl ActiveTemplatePin {
    fn from_template(process_id: impl Into<String>, template: &CompanyTemplateVersion) -> Self {
        let process_id = process_id.into();
        let authority_digest = json_digest(&json!({
            "process_id": process_id,
            "template_id": template.template_id,
            "template_version": template.version,
            "template_hash": template.template_hash,
            "policy_profile_id": template.policy_profile_id,
            "policy_profile_version": template.policy_profile_version,
            "policy_profile_digest": template.policy_profile_digest,
        }));
        Self {
            schema: COMPANY_TEMPLATE_REGISTRY_SCHEMA.to_owned(),
            process_id,
            template_id: template.template_id.clone(),
            template_version: template.version,
            template_hash: template.template_hash.clone(),
            policy_profile_id: template.policy_profile_id.clone(),
            policy_profile_version: template.policy_profile_version,
            policy_profile_digest: template.policy_profile_digest.clone(),
            authority_digest,
        }
    }

    fn validate_against(&self, template: &CompanyTemplateVersion) -> Result<(), &'static str> {
        required(&self.process_id, "company_template_process_required")?;
        if self.schema != COMPANY_TEMPLATE_REGISTRY_SCHEMA
            || self.template_id != template.template_id
            || self.template_version != template.version
            || self.template_hash != template.template_hash
            || self.policy_profile_id != template.policy_profile_id
            || self.policy_profile_version != template.policy_profile_version
            || self.policy_profile_digest != template.policy_profile_digest
        {
            return Err("company_template_process_pin_drift");
        }
        digest(
            &self.authority_digest,
            "company_template_authority_digest_invalid",
        )?;
        if self.authority_digest
            != Self::from_template(self.process_id.clone(), template).authority_digest
        {
            return Err("company_template_authority_digest_mismatch");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyTemplateConfigProposal {
    pub schema: String,
    pub proposal_id: String,
    pub template_id: String,
    pub current_version: u64,
    pub current_template_hash: String,
    pub new_template: CompanyTemplateVersion,
    pub active_process_ids: Vec<String>,
    pub migration_required: bool,
    pub source: CompanyConfigSource,
    /// Prompt annotations are intentionally non-authoritative and must stay empty on a proposal.
    pub prompt_allowed_tools: Vec<String>,
    pub approval_ref: Option<String>,
    pub acceptance_ref: Option<String>,
    pub status: CompanyConfigProposalStatus,
    pub digest: String,
}

impl CompanyTemplateConfigProposal {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_TEMPLATE_PROPOSAL_SCHEMA
            || self.current_version == 0
            || self.new_template.version <= self.current_version
            || self.template_id != self.new_template.template_id
        {
            return Err("company_template_proposal_header_invalid");
        }
        required(&self.proposal_id, "company_template_proposal_id_required")?;
        digest(
            &self.current_template_hash,
            "company_template_current_hash_invalid",
        )?;
        self.new_template.validate()?;
        if self.new_template.status != CompanyTemplateStatus::Active {
            return Err("company_template_proposal_template_not_active");
        }
        if self.prompt_allowed_tools.iter().any(|tool| {
            tool.trim().is_empty() || tool.len() > 256 || tool.contains(['\0', '\r', '\n'])
        }) {
            return Err("company_template_prompt_tools_invalid");
        }
        if !self.prompt_allowed_tools.is_empty() {
            return Err("company_template_prompt_tools_not_authority");
        }
        self.source.validate_authority_boundary()?;
        let mut ids = BTreeSet::new();
        for process_id in &self.active_process_ids {
            required(process_id, "company_template_active_process_invalid")?;
            if !ids.insert(process_id) {
                return Err("company_template_active_process_duplicate");
            }
        }
        optional_ref(&self.approval_ref, "company_template_approval_ref_invalid")?;
        optional_ref(
            &self.acceptance_ref,
            "company_template_acceptance_ref_invalid",
        )?;
        if self.status == CompanyConfigProposalStatus::Approved
            && (self.approval_ref.is_none() || self.acceptance_ref.is_none())
        {
            return Err("company_template_approval_and_acceptance_required");
        }
        digest(&self.digest, "company_template_proposal_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_template_proposal_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "proposal_id": self.proposal_id,
            "template_id": self.template_id,
            "current_version": self.current_version,
            "current_template_hash": self.current_template_hash,
            "new_template": self.new_template,
            "active_process_ids": self.active_process_ids,
            "migration_required": self.migration_required,
            "source": self.source,
            "prompt_allowed_tools": self.prompt_allowed_tools,
            "approval_ref": self.approval_ref,
            "acceptance_ref": self.acceptance_ref,
            "status": self.status,
        }))
    }
}

pub type CompanyTemplateUpgradeProposal = CompanyTemplateConfigProposal;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyTemplateRollbackRecord {
    pub schema: String,
    pub rollback_id: String,
    pub template_id: String,
    pub from_version: u64,
    pub from_template_hash: String,
    pub to_version: u64,
    pub to_template_hash: String,
    pub approval_ref: String,
    pub reason: String,
    pub active_process_ids: Vec<String>,
    pub digest: String,
}

impl CompanyTemplateRollbackRecord {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_TEMPLATE_ROLLBACK_SCHEMA
            || self.from_version == 0
            || self.to_version == 0
            || self.to_version >= self.from_version
        {
            return Err("company_template_rollback_header_invalid");
        }
        for (value, field) in [
            (&self.rollback_id, "company_template_rollback_id_required"),
            (
                &self.template_id,
                "company_template_rollback_template_required",
            ),
            (
                &self.approval_ref,
                "company_template_rollback_approval_required",
            ),
            (&self.reason, "company_template_rollback_reason_required"),
        ] {
            required(value, field)?;
        }
        digest(
            &self.from_template_hash,
            "company_template_rollback_from_hash_invalid",
        )?;
        digest(
            &self.to_template_hash,
            "company_template_rollback_to_hash_invalid",
        )?;
        unique(
            &self.active_process_ids,
            "company_template_rollback_processes_invalid",
            4_096,
        )
        .or_else(|error| {
            if self.active_process_ids.is_empty() {
                Ok(())
            } else {
                Err(error)
            }
        })?;
        digest(&self.digest, "company_template_rollback_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_template_rollback_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "rollback_id": self.rollback_id,
            "template_id": self.template_id,
            "from_version": self.from_version,
            "from_template_hash": self.from_template_hash,
            "to_version": self.to_version,
            "to_template_hash": self.to_template_hash,
            "approval_ref": self.approval_ref,
            "reason": self.reason,
            "active_process_ids": self.active_process_ids,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyTemplateRegistry {
    pub schema: String,
    pub revision: u64,
    pub role_packs: BTreeMap<String, BTreeMap<u64, CompanyRolePackVersion>>,
    pub policy_profiles: BTreeMap<String, BTreeMap<u64, PolicyProfile>>,
    pub templates: BTreeMap<String, BTreeMap<u64, CompanyTemplateVersion>>,
    pub active_versions: BTreeMap<String, u64>,
    pub process_pins: BTreeMap<String, ActiveTemplatePin>,
    pub proposals: BTreeMap<String, CompanyTemplateConfigProposal>,
    pub rollback_records: BTreeMap<String, CompanyTemplateRollbackRecord>,
}

impl Default for CompanyTemplateRegistry {
    fn default() -> Self {
        Self {
            schema: COMPANY_TEMPLATE_REGISTRY_SCHEMA.to_owned(),
            revision: 0,
            role_packs: BTreeMap::new(),
            policy_profiles: BTreeMap::new(),
            templates: BTreeMap::new(),
            active_versions: BTreeMap::new(),
            process_pins: BTreeMap::new(),
            proposals: BTreeMap::new(),
            rollback_records: BTreeMap::new(),
        }
    }
}

impl CompanyTemplateRegistry {
    fn bump(&mut self) -> Result<(), &'static str> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("company_template_registry_revision_exhausted")?;
        Ok(())
    }

    fn validate_template_references(
        &self,
        template: &CompanyTemplateVersion,
    ) -> Result<(), &'static str> {
        template.validate()?;
        for role in &template.role_packs {
            let installed = self
                .role_packs
                .get(&role.role_id)
                .and_then(|versions| versions.get(&role.version))
                .filter(|installed| {
                    installed.role_id == role.role_id && installed.digest == role.digest
                })
                .ok_or("company_template_role_pack_not_installed")?;
            if installed.policy_profile_id != template.policy_profile_id
                || installed.policy_profile_version != template.policy_profile_version
                || installed.policy_profile_digest != template.policy_profile_digest
            {
                return Err("company_template_role_pack_policy_mismatch");
            }
        }
        let profile = self
            .policy_profiles
            .get(&template.policy_profile_id)
            .and_then(|versions| versions.get(&template.policy_profile_version))
            .ok_or("company_template_policy_profile_not_installed")?;
        profile
            .validate()
            .map_err(|_| "company_template_policy_profile_invalid")?;
        if profile.status != PolicyProfileStatus::Active
            || profile.profile_digest != template.policy_profile_digest
        {
            return Err("company_template_policy_profile_not_active");
        }
        Ok(())
    }

    fn current_process_ids(&self, template_id: &str, version: u64) -> Vec<String> {
        self.process_pins
            .iter()
            .filter(|(_, pin)| pin.template_id == template_id && pin.template_version == version)
            .map(|(process_id, _)| process_id.clone())
            .collect()
    }

    fn template(
        &self,
        template_id: &str,
        version: u64,
    ) -> Result<&CompanyTemplateVersion, &'static str> {
        self.templates
            .get(template_id)
            .and_then(|versions| versions.get(&version))
            .ok_or("company_template_not_found")
    }

    pub fn install_role_pack(
        &mut self,
        role_pack: CompanyRolePackVersion,
    ) -> Result<(), &'static str> {
        role_pack.validate()?;
        let versions = self
            .role_packs
            .entry(role_pack.role_id.clone())
            .or_default();
        if let Some(existing) = versions.get(&role_pack.version) {
            if existing.role_id == role_pack.role_id && existing.digest == role_pack.digest {
                return Ok(());
            }
            return Err("company_role_pack_version_conflict");
        }
        versions.insert(role_pack.version, role_pack);
        self.bump()
    }

    pub fn install_policy_profile(&mut self, profile: PolicyProfile) -> Result<(), &'static str> {
        profile
            .validate()
            .map_err(|_| "company_template_policy_profile_invalid")?;
        let key = profile.profile_id.to_string();
        let versions = self.policy_profiles.entry(key).or_default();
        if let Some(existing) = versions.get(&profile.version) {
            if existing.profile_digest == profile.profile_digest {
                return Ok(());
            }
            return Err("company_template_policy_profile_version_conflict");
        }
        versions.insert(profile.version, profile);
        self.bump()
    }

    pub fn install_template(
        &mut self,
        template: CompanyTemplateVersion,
    ) -> Result<(), &'static str> {
        if self.templates.contains_key(&template.template_id) {
            return Err("company_template_upgrade_requires_proposal");
        }
        if template.status != CompanyTemplateStatus::Active {
            return Err("company_template_initial_install_must_be_active");
        }
        self.validate_template_references(&template)?;
        let template_id = template.template_id.clone();
        let version = template.version;
        self.templates
            .entry(template_id.clone())
            .or_default()
            .insert(version, template);
        self.active_versions.insert(template_id, version);
        self.bump()
    }

    pub fn pin_process(
        &mut self,
        process_id: impl Into<String>,
        template_id: &str,
    ) -> Result<ActiveTemplatePin, &'static str> {
        let process_id = process_id.into();
        required(&process_id, "company_template_process_required")?;
        let version = *self
            .active_versions
            .get(template_id)
            .ok_or("company_template_not_active")?;
        let template = self.template(template_id, version)?;
        let pin = ActiveTemplatePin::from_template(process_id.clone(), template);
        if let Some(existing) = self.process_pins.get(&process_id) {
            if existing == &pin {
                return Ok(pin);
            }
            return Err("company_template_process_already_pinned");
        }
        self.process_pins.insert(process_id, pin.clone());
        self.bump()?;
        Ok(pin)
    }

    pub fn propose_upgrade(
        &mut self,
        mut proposal: CompanyTemplateConfigProposal,
    ) -> Result<(), &'static str> {
        proposal.validate()?;
        if self.proposals.contains_key(&proposal.proposal_id) {
            if self.proposals[&proposal.proposal_id].digest == proposal.digest {
                return Ok(());
            }
            return Err("company_template_proposal_conflict");
        }
        let current_version = *self
            .active_versions
            .get(&proposal.template_id)
            .ok_or("company_template_not_active")?;
        if current_version != proposal.current_version {
            return Err("company_template_upgrade_current_version_stale");
        }
        let current = self.template(&proposal.template_id, current_version)?;
        if current.template_hash != proposal.current_template_hash {
            return Err("company_template_upgrade_current_hash_stale");
        }
        if self
            .templates
            .get(&proposal.template_id)
            .is_some_and(|versions| versions.contains_key(&proposal.new_template.version))
        {
            return Err("company_template_version_already_installed");
        }
        self.validate_template_references(&proposal.new_template)?;
        let mut expected_processes =
            self.current_process_ids(&proposal.template_id, current_version);
        expected_processes.sort();
        let mut supplied_processes = proposal.active_process_ids.clone();
        supplied_processes.sort();
        if supplied_processes != expected_processes {
            return Err("company_template_active_process_snapshot_stale");
        }
        proposal.active_process_ids = supplied_processes;
        proposal.digest = proposal.canonical_digest();
        proposal.validate()?;
        self.proposals
            .insert(proposal.proposal_id.clone(), proposal);
        self.bump()
    }

    pub fn approve_upgrade(
        &mut self,
        proposal_id: &str,
        approval_ref: impl Into<String>,
        acceptance_ref: impl Into<String>,
    ) -> Result<(), &'static str> {
        let approval_ref = approval_ref.into();
        let acceptance_ref = acceptance_ref.into();
        required(&approval_ref, "company_template_approval_required")?;
        required(&acceptance_ref, "company_template_acceptance_required")?;
        let mut proposal = self
            .proposals
            .get(proposal_id)
            .cloned()
            .ok_or("company_template_proposal_not_found")?;
        if proposal.status == CompanyConfigProposalStatus::Approved {
            if proposal.approval_ref.as_deref() == Some(approval_ref.as_str())
                && proposal.acceptance_ref.as_deref() == Some(acceptance_ref.as_str())
            {
                return Ok(());
            }
            return Err("company_template_proposal_already_approved");
        }
        if proposal.status != CompanyConfigProposalStatus::Proposed {
            return Err("company_template_proposal_not_approvable");
        }
        let current_version = *self
            .active_versions
            .get(&proposal.template_id)
            .ok_or("company_template_not_active")?;
        let current = self.template(&proposal.template_id, current_version)?;
        if current_version != proposal.current_version
            || current.template_hash != proposal.current_template_hash
        {
            return Err("company_template_upgrade_current_state_stale");
        }
        let mut expected_processes =
            self.current_process_ids(&proposal.template_id, current_version);
        expected_processes.sort();
        if proposal.active_process_ids != expected_processes {
            return Err("company_template_active_process_snapshot_stale");
        }
        if !proposal.active_process_ids.is_empty() && !proposal.migration_required {
            return Err("company_template_active_process_requires_migration");
        }
        self.validate_template_references(&proposal.new_template)?;
        let new_version = proposal.new_template.version;
        let mut next_template = proposal.new_template.clone();
        next_template.status = CompanyTemplateStatus::Active;
        self.templates
            .entry(proposal.template_id.clone())
            .or_default()
            .insert(new_version, next_template);
        if let Some(old) = self
            .templates
            .get_mut(&proposal.template_id)
            .and_then(|versions| versions.get_mut(&current_version))
        {
            old.status = CompanyTemplateStatus::Retired;
        }
        self.active_versions
            .insert(proposal.template_id.clone(), new_version);
        proposal.approval_ref = Some(approval_ref);
        proposal.acceptance_ref = Some(acceptance_ref);
        proposal.status = CompanyConfigProposalStatus::Approved;
        proposal.digest = proposal.canonical_digest();
        proposal.validate()?;
        self.proposals.insert(proposal_id.to_owned(), proposal);
        self.bump()
    }

    pub fn migrate_process(
        &mut self,
        process_id: &str,
        proposal_id: &str,
    ) -> Result<ActiveTemplatePin, &'static str> {
        let proposal = self
            .proposals
            .get(proposal_id)
            .ok_or("company_template_proposal_not_found")?;
        if proposal.status != CompanyConfigProposalStatus::Approved {
            return Err("company_template_migration_requires_approved_proposal");
        }
        if !proposal.migration_required
            || !proposal
                .active_process_ids
                .iter()
                .any(|id| id == process_id)
        {
            return Err("company_template_process_migration_not_authorized");
        }
        let existing = self
            .process_pins
            .get(process_id)
            .cloned()
            .ok_or("company_template_process_pin_missing")?;
        if existing.template_id != proposal.template_id
            || existing.template_version != proposal.current_version
            || existing.template_hash != proposal.current_template_hash
        {
            return Err("company_template_process_pin_stale");
        }
        let template = self.template(&proposal.template_id, proposal.new_template.version)?;
        let pin = ActiveTemplatePin::from_template(process_id.to_owned(), template);
        self.process_pins.insert(process_id.to_owned(), pin.clone());
        self.bump()?;
        Ok(pin)
    }

    pub fn rollback_template(
        &mut self,
        record: CompanyTemplateRollbackRecord,
    ) -> Result<(), &'static str> {
        record.validate()?;
        if let Some(existing) = self.rollback_records.get(&record.rollback_id) {
            if existing.digest == record.digest {
                return Ok(());
            }
            return Err("company_template_rollback_conflict");
        }
        let current_version = *self
            .active_versions
            .get(&record.template_id)
            .ok_or("company_template_not_active")?;
        if current_version != record.from_version {
            return Err("company_template_rollback_current_version_stale");
        }
        let from = self.template(&record.template_id, record.from_version)?;
        let to = self.template(&record.template_id, record.to_version)?;
        if from.template_hash != record.from_template_hash
            || to.template_hash != record.to_template_hash
        {
            return Err("company_template_rollback_hash_stale");
        }
        let mut expected_processes =
            self.current_process_ids(&record.template_id, record.from_version);
        expected_processes.sort();
        let mut supplied_processes = record.active_process_ids.clone();
        supplied_processes.sort();
        if supplied_processes != expected_processes {
            return Err("company_template_rollback_process_snapshot_stale");
        }
        if let Some(target) = self
            .templates
            .get_mut(&record.template_id)
            .and_then(|versions| versions.get_mut(&record.to_version))
        {
            target.status = CompanyTemplateStatus::Active;
        }
        if let Some(previous) = self
            .templates
            .get_mut(&record.template_id)
            .and_then(|versions| versions.get_mut(&record.from_version))
        {
            previous.status = CompanyTemplateStatus::Retired;
        }
        self.active_versions
            .insert(record.template_id.clone(), record.to_version);
        self.rollback_records
            .insert(record.rollback_id.clone(), record);
        self.bump()
    }

    pub fn active_template(
        &self,
        template_id: &str,
    ) -> Result<&CompanyTemplateVersion, &'static str> {
        let version = *self
            .active_versions
            .get(template_id)
            .ok_or("company_template_not_active")?;
        self.template(template_id, version)
    }

    pub fn pinned_template(
        &self,
        process_id: &str,
    ) -> Result<&CompanyTemplateVersion, &'static str> {
        let pin = self
            .process_pins
            .get(process_id)
            .ok_or("company_template_process_pin_missing")?;
        let template = self.template(&pin.template_id, pin.template_version)?;
        pin.validate_against(template)?;
        Ok(template)
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "revision": self.revision,
            "role_packs": self.role_packs,
            "policy_profiles": self.policy_profiles,
            "templates": self.templates,
            "active_versions": self.active_versions,
            "process_pins": self.process_pins,
            "proposals": self.proposals,
            "rollback_records": self.rollback_records,
        }))
    }
}
