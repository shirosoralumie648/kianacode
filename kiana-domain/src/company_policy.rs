//! Company command authorization and human-decision contracts.
//!
//! These values describe the decision boundary; they do not execute commands or mint capability
//! grants. ControlPlane builds a decision from its authenticated RequestContext and the current
//! Company revision, then persists it in the same Company event as the command.

use crate::{
    json_digest, CompanyBusinessAction, CompanyCommand, CompanyProof, RequestContext, RequestId,
    SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const COMPANY_COMMAND_POLICY_SCHEMA: &str = "kiana.company-command-policy.v1";
pub const HUMAN_TASK_SCHEMA: &str = "kiana.human-task.v1";
pub const HUMAN_DECISION_SCHEMA: &str = "kiana.human-decision.v1";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionActorKind {
    Human,
    Agent,
    Service,
    #[default]
    Unknown,
}

impl DecisionActorKind {
    pub fn from_context(context: &RequestContext) -> Self {
        if context.cell_id.is_some() {
            Self::Agent
        } else if context
            .actor_id
            .as_deref()
            .is_some_and(|actor| actor.starts_with("service:") || actor.starts_with("system:"))
        {
            Self::Service
        } else if context
            .actor_id
            .as_deref()
            .is_some_and(|actor| !actor.trim().is_empty())
        {
            Self::Human
        } else {
            Self::Unknown
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionPurpose {
    None,
    ObjectiveApproval,
    ProjectApproval,
    BudgetApproval,
    PacketApproval,
    AcceptanceDecision,
    DeliveryApproval,
    DeliveryConfirmation,
    ChangeApproval,
    ProjectControl,
    BusinessCloseout,
}

impl DecisionPurpose {
    pub fn requires_human(self) -> bool {
        matches!(
            self,
            Self::ObjectiveApproval
                | Self::ProjectApproval
                | Self::BudgetApproval
                | Self::AcceptanceDecision
                | Self::DeliveryApproval
                | Self::DeliveryConfirmation
                | Self::ChangeApproval
                | Self::BusinessCloseout
        )
    }

    pub fn allowed_options(self) -> &'static [DecisionOption] {
        match self {
            Self::None => &[],
            Self::AcceptanceDecision => &[
                DecisionOption::Accept,
                DecisionOption::Reject,
                DecisionOption::Waive,
            ],
            Self::DeliveryConfirmation => &[DecisionOption::Confirm, DecisionOption::Reject],
            Self::ProjectControl => &[
                DecisionOption::Approve,
                DecisionOption::Reject,
                DecisionOption::Cancel,
            ],
            _ => &[DecisionOption::Approve, DecisionOption::Reject],
        }
    }

    pub fn default_option(self) -> DecisionOption {
        match self {
            Self::DeliveryConfirmation => DecisionOption::Confirm,
            _ => DecisionOption::Approve,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionOption {
    Approve,
    Reject,
    Accept,
    Waive,
    Confirm,
    Cancel,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyCommandPolicy {
    pub schema: String,
    pub command_name: String,
    pub allowed_roles: Vec<String>,
    pub actor_kinds: Vec<DecisionActorKind>,
    pub assignment_required: bool,
    pub purpose: DecisionPurpose,
    pub evidence_required: bool,
    pub policy_digest: String,
}

impl CompanyCommandPolicy {
    pub fn for_command(command: &CompanyCommand) -> Self {
        let purpose = command_decision_purpose(command);
        let allowed_roles = command_policy_roles(command);
        let actor_kinds = if purpose.requires_human() {
            vec![DecisionActorKind::Human]
        } else if allowed_roles.is_empty() {
            Vec::new()
        } else {
            vec![
                DecisionActorKind::Human,
                DecisionActorKind::Agent,
                DecisionActorKind::Service,
            ]
        };
        let mut policy = Self {
            schema: COMPANY_COMMAND_POLICY_SCHEMA.to_owned(),
            command_name: command.event_name().to_owned(),
            allowed_roles,
            actor_kinds,
            assignment_required: true,
            purpose,
            evidence_required: !matches!(purpose, DecisionPurpose::None),
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy
    }

    pub fn deny(command_name: impl Into<String>) -> Self {
        let mut policy = Self {
            schema: COMPANY_COMMAND_POLICY_SCHEMA.to_owned(),
            command_name: command_name.into(),
            allowed_roles: Vec::new(),
            actor_kinds: Vec::new(),
            assignment_required: true,
            purpose: DecisionPurpose::None,
            evidence_required: true,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_COMMAND_POLICY_SCHEMA
            || self.command_name.trim().is_empty()
            || self.policy_digest != self.digest()
            || (self.allowed_roles.is_empty() && !self.actor_kinds.is_empty())
            || self
                .allowed_roles
                .iter()
                .any(|role| role.trim().is_empty() || role.len() > 128)
        {
            return Err("company_command_policy_invalid".to_owned());
        }
        let mut roles = BTreeSet::new();
        if self
            .allowed_roles
            .iter()
            .any(|role| !roles.insert(role.clone()))
        {
            return Err("company_command_policy_duplicate_role".to_owned());
        }
        if !self.purpose.requires_human()
            && self
                .actor_kinds
                .iter()
                .any(|kind| *kind == DecisionActorKind::Unknown)
        {
            return Err("company_command_policy_unknown_actor".to_owned());
        }
        Ok(())
    }

    pub fn authorize_context(&self, context: &RequestContext) -> Result<(), &'static str> {
        self.validate()
            .map_err(|_| "company_command_policy_invalid")?;
        if !self
            .allowed_roles
            .iter()
            .any(|role| role == &context.role_id)
        {
            return Err("company_role_denied");
        }
        if context
            .actor_id
            .as_deref()
            .is_none_or(|actor| actor.trim().is_empty())
        {
            return Err("company_actor_required");
        }
        let actor_kind = DecisionActorKind::from_context(context);
        if !self.actor_kinds.contains(&actor_kind) {
            return Err(if self.purpose.requires_human() {
                "company_human_decision_required"
            } else {
                "company_actor_kind_denied"
            });
        }
        Ok(())
    }

    pub fn decision(
        &self,
        context: &RequestContext,
        command: &CompanyCommand,
        expected_revision: u64,
        now_unix_ms: u64,
    ) -> Result<Option<HumanDecision>, String> {
        if !self.purpose.requires_human() {
            return Ok(None);
        }
        self.authorize_context(context).map_err(str::to_owned)?;
        HumanDecision::new(
            RequestId::new(),
            self.purpose,
            command_decision_option(command, self.purpose),
            context,
            command.event_name(),
            expected_revision,
            json_digest(&json!(command)),
            now_unix_ms,
        )
        .map(Some)
    }

    /// Revalidate the server-created human decision at the Company transaction boundary.
    ///
    /// A decision is an input to authorization, not a receipt that a caller may copy into a
    /// command payload. Binding the command digest, expected revision, authenticated scope and
    /// expiry here keeps approval and acceptance subjects separate from the business transition.
    pub fn validate_decision_for(
        &self,
        decision: Option<&HumanDecision>,
        context: &RequestContext,
        command: &CompanyCommand,
        expected_revision: u64,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        let Some(decision) = decision else {
            return if self.purpose.requires_human() {
                Err("company_human_decision_missing".to_owned())
            } else {
                Ok(())
            };
        };
        if !self.purpose.requires_human() {
            return Err("company_unexpected_human_decision".to_owned());
        }
        decision.validate()?;
        let actor = context
            .actor_id
            .as_deref()
            .filter(|actor| !actor.trim().is_empty())
            .ok_or_else(|| "company_human_decision_actor_missing".to_owned())?;
        let expected_scope = json_digest(&json!({
            "actor": actor,
            "project_root": context.project_root,
            "role_id": context.role_id,
            "department_id": context.department_id,
            "session_id": context.session_id,
        }));
        if decision.decider_principal_id != actor
            || decision.actor_kind != DecisionActorKind::Human
            || decision.role_id != context.role_id
            || decision.session_id != context.session_id
            || decision.command_name != command.event_name()
            || decision.target_revision != expected_revision
            || decision.target_digest != json_digest(&json!(command))
            || decision.scope_digest != expected_scope
            || !decision.active_at(now_unix_ms)
        {
            return Err("company_human_decision_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "command_name": self.command_name,
            "allowed_roles": self.allowed_roles,
            "actor_kinds": self.actor_kinds,
            "assignment_required": self.assignment_required,
            "purpose": self.purpose,
            "evidence_required": self.evidence_required,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanDecision {
    pub schema: String,
    pub decision_id: RequestId,
    pub purpose: DecisionPurpose,
    pub option: DecisionOption,
    pub decider_principal_id: String,
    pub actor_kind: DecisionActorKind,
    pub role_id: String,
    pub session_id: SessionId,
    pub command_name: String,
    pub target_revision: u64,
    pub target_digest: String,
    pub scope_digest: String,
    pub decided_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub authority_epoch: u64,
}

impl HumanDecision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        decision_id: RequestId,
        purpose: DecisionPurpose,
        option: DecisionOption,
        context: &RequestContext,
        command_name: &str,
        target_revision: u64,
        target_digest: String,
        decided_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let actor_kind = DecisionActorKind::from_context(context);
        if !purpose.requires_human()
            || actor_kind != DecisionActorKind::Human
            || !purpose.allowed_options().contains(&option)
            || target_revision == 0
            || decided_at_unix_ms == 0
            || command_name.trim().is_empty()
        {
            return Err("human_decision_invalid".to_owned());
        }
        let decider_principal_id = context
            .actor_id
            .clone()
            .ok_or_else(|| "human_decision_actor_required".to_owned())?;
        let scope_digest = json_digest(&json!({
            "actor": decider_principal_id,
            "project_root": context.project_root,
            "role_id": context.role_id,
            "department_id": context.department_id,
            "session_id": context.session_id,
        }));
        let decision = Self {
            schema: HUMAN_DECISION_SCHEMA.to_owned(),
            decision_id,
            purpose,
            option,
            decider_principal_id,
            actor_kind,
            role_id: context.role_id.clone(),
            session_id: context.session_id.clone(),
            command_name: command_name.to_owned(),
            target_revision,
            target_digest,
            scope_digest,
            decided_at_unix_ms,
            expires_at_unix_ms: decided_at_unix_ms.saturating_add(300_000),
            authority_epoch: 1,
        };
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HUMAN_DECISION_SCHEMA
            || self.actor_kind != DecisionActorKind::Human
            || self.decider_principal_id.trim().is_empty()
            || self.role_id.trim().is_empty()
            || self.session_id.is_empty()
            || self.command_name.trim().is_empty()
            || self.target_revision == 0
            || self.decided_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.decided_at_unix_ms
            || self.authority_epoch == 0
        {
            return Err("human_decision_invalid".to_owned());
        }
        for (digest, field) in [
            (&self.target_digest, "human_decision_target_digest"),
            (&self.scope_digest, "human_decision_scope_digest"),
        ] {
            let Some(hex) = digest.strip_prefix("sha256:") else {
                return Err(format!("{field}_invalid"));
            };
            if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("{field}_invalid"));
            }
        }
        if !self.purpose.allowed_options().contains(&self.option) {
            return Err("human_decision_option_invalid".to_owned());
        }
        Ok(())
    }

    pub fn active_at(&self, now_unix_ms: u64) -> bool {
        now_unix_ms >= self.decided_at_unix_ms && now_unix_ms < self.expires_at_unix_ms
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanTaskStatus {
    Pending,
    Decided,
    Expired,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTask {
    pub schema: String,
    pub task_id: String,
    pub purpose: DecisionPurpose,
    pub target_kind: String,
    pub target_id: String,
    pub target_revision: u64,
    pub target_digest: String,
    pub scope_digest: String,
    pub created_by: String,
    pub assigned_to: Option<String>,
    pub allowed_options: Vec<DecisionOption>,
    pub expires_at_unix_ms: u64,
    pub authority_epoch: u64,
    pub status: HumanTaskStatus,
    pub decision: Option<HumanDecision>,
}

impl HumanTask {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HUMAN_TASK_SCHEMA
            || self.task_id.trim().is_empty()
            || self.target_kind.trim().is_empty()
            || self.target_id.trim().is_empty()
            || self.task_id.len() > 256
            || self.target_kind.len() > 128
            || self.target_id.len() > 256
            || self.target_revision == 0
            || self.created_by.trim().is_empty()
            || self.created_by.len() > 256
            || self.allowed_options.is_empty()
            || self.expires_at_unix_ms == 0
            || self.authority_epoch == 0
        {
            return Err("human_task_invalid".to_owned());
        }
        for (digest, field) in [
            (&self.target_digest, "human_task_target_digest"),
            (&self.scope_digest, "human_task_scope_digest"),
        ] {
            let Some(hex) = digest.strip_prefix("sha256:") else {
                return Err(format!("{field}_invalid"));
            };
            if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("{field}_invalid"));
            }
        }
        let expected = self.purpose.allowed_options();
        if self
            .allowed_options
            .iter()
            .any(|option| !expected.contains(option))
        {
            return Err("human_task_options_invalid".to_owned());
        }
        if let Some(decision) = &self.decision {
            decision.validate()?;
            if decision.purpose != self.purpose
                || decision.target_revision != self.target_revision
                || decision.target_digest != self.target_digest
                || !self.allowed_options.contains(&decision.option)
            {
                return Err("human_task_decision_mismatch".to_owned());
            }
        }
        Ok(())
    }
}

fn command_decision_purpose(command: &CompanyCommand) -> DecisionPurpose {
    match command {
        CompanyCommand::Business { action, .. } => match action.as_ref() {
            CompanyBusinessAction::Closeout { action, .. } => match action.as_ref() {
                crate::BusinessCloseoutAction::ApproveDelivery { .. } => {
                    DecisionPurpose::DeliveryApproval
                }
                crate::BusinessCloseoutAction::ConfirmDelivery { .. } => {
                    DecisionPurpose::DeliveryConfirmation
                }
                crate::BusinessCloseoutAction::DecideChange { .. } => {
                    DecisionPurpose::ChangeApproval
                }
                crate::BusinessCloseoutAction::AchieveObjective { .. } => {
                    DecisionPurpose::ObjectiveApproval
                }
                _ => DecisionPurpose::None,
            },
            CompanyBusinessAction::ApproveCharter { .. } => DecisionPurpose::ProjectApproval,
            CompanyBusinessAction::DecideAcceptance { .. } => DecisionPurpose::AcceptanceDecision,
            CompanyBusinessAction::RevokeAssignment { .. }
            | CompanyBusinessAction::CreateOrganization { .. }
            | CompanyBusinessAction::Appoint { .. }
            | CompanyBusinessAction::Pause { .. }
            | CompanyBusinessAction::Resume { .. } => DecisionPurpose::ProjectControl,
            _ => DecisionPurpose::None,
        },
        CompanyCommand::DecideObjective { .. } => DecisionPurpose::ObjectiveApproval,
        CompanyCommand::ConfigureBudget { .. } => DecisionPurpose::BudgetApproval,
        CompanyCommand::ApproveProject { .. } | CompanyCommand::RejectProject { .. } => {
            DecisionPurpose::ProjectApproval
        }
        CompanyCommand::ApprovePacket { .. } => DecisionPurpose::PacketApproval,
        CompanyCommand::DecideAcceptance { .. } => DecisionPurpose::AcceptanceDecision,
        CompanyCommand::ApproveDelivery { .. } => DecisionPurpose::DeliveryApproval,
        CompanyCommand::ConfirmDelivery { .. } => DecisionPurpose::DeliveryConfirmation,
        CompanyCommand::DecideChange { .. } => DecisionPurpose::ChangeApproval,
        CompanyCommand::PauseProject { .. }
        | CompanyCommand::ResumeProject { .. }
        | CompanyCommand::RequestCancelProject { .. }
        | CompanyCommand::ConfirmCancelProject { .. }
        | CompanyCommand::FailProject { .. }
        | CompanyCommand::ArchiveProject { .. } => DecisionPurpose::ProjectControl,
        _ => DecisionPurpose::None,
    }
}

fn command_decision_option(command: &CompanyCommand, purpose: DecisionPurpose) -> DecisionOption {
    match command {
        CompanyCommand::Business { action, .. } => match action.as_ref() {
            CompanyBusinessAction::DecideAcceptance { decision, .. } => match decision {
                crate::AcceptanceDecision::Accept => DecisionOption::Accept,
                crate::AcceptanceDecision::Reject => DecisionOption::Reject,
                crate::AcceptanceDecision::Waive => DecisionOption::Waive,
            },
            _ => purpose.default_option(),
        },
        CompanyCommand::DecideObjective { approve: false, .. }
        | CompanyCommand::RejectProject { .. } => DecisionOption::Reject,
        CompanyCommand::DecideAcceptance {
            decision: crate::AcceptanceDecision::Accept,
            ..
        } => DecisionOption::Accept,
        CompanyCommand::DecideAcceptance {
            decision: crate::AcceptanceDecision::Reject,
            ..
        } => DecisionOption::Reject,
        CompanyCommand::DecideAcceptance {
            decision: crate::AcceptanceDecision::Waive,
            ..
        } => DecisionOption::Waive,
        CompanyCommand::ConfirmDelivery { .. } => DecisionOption::Confirm,
        CompanyCommand::RequestCancelProject { .. }
        | CompanyCommand::ConfirmCancelProject { .. } => DecisionOption::Cancel,
        _ => purpose.default_option(),
    }
}

fn command_policy_roles(command: &CompanyCommand) -> Vec<String> {
    let roles: &[&str] = match command {
        CompanyCommand::Business { action, .. } => action.roles(),
        CompanyCommand::RegisterArtifact { .. } => {
            &["builder", "reviewer", "architect", "pm", "closer"]
        }
        CompanyCommand::AdvanceInitiative { .. } => &["sponsor", "pm"],
        CompanyCommand::AcknowledgeHandoff { .. } => &["builder", "pm"],
        CompanyCommand::ReconcileRun { .. } | CompanyCommand::RecordRunStarted { .. } => {
            &["builder", "closer"]
        }
        CompanyCommand::ConfigureBudget { .. }
        | CompanyCommand::ProposeObjective { .. }
        | CompanyCommand::DecideObjective { .. }
        | CompanyCommand::AchieveObjective { .. }
        | CompanyCommand::ApproveProject { .. }
        | CompanyCommand::RejectProject { .. }
        | CompanyCommand::DecideChange { .. } => &["sponsor"],
        CompanyCommand::ClaimPacket { .. } | CompanyCommand::RenewPacketClaim { .. } => {
            &["builder"]
        }
        // CP-23: the expired-lease scan is a maintenance mutation; without an explicit entry the
        // fallback is an empty role set and `authorize_context` denies every subject, which would
        // make the documented reclaim path unreachable.
        CompanyCommand::ReclaimPacketClaim { .. } => &["builder", "closer"],
        CompanyCommand::StartRun { .. } | CompanyCommand::RequestAcceptance { .. } => {
            &["builder", "closer"]
        }
        CompanyCommand::ReviewPacket { .. } | CompanyCommand::RecordReview { .. } => &["reviewer"],
        CompanyCommand::DecideAcceptance { .. } => &["reviewer", "sponsor"],
        CompanyCommand::PrepareDelivery { .. }
        | CompanyCommand::ConfirmDelivery { .. }
        | CompanyCommand::ApproveDelivery { .. }
        | CompanyCommand::Deliver { .. }
        | CompanyCommand::CloseProject { .. }
        | CompanyCommand::RecordOutcome { .. }
        | CompanyCommand::MarkDeliveryUnknown { .. }
        | CompanyCommand::ReconcileDelivery { .. } => &["closer", "sponsor"],
        CompanyCommand::CreateMilestone { .. }
        | CompanyCommand::ApprovePacket { .. }
        | CompanyCommand::ReworkPacket { .. }
        | CompanyCommand::PlanProject { .. } => &["pm"],
        CompanyCommand::ProposeProject { .. }
        | CompanyCommand::StartChartering { .. }
        | CompanyCommand::PauseProject { .. }
        | CompanyCommand::ResumeProject { .. }
        | CompanyCommand::RequestChange { .. }
        | CompanyCommand::RecordDirectIntake { .. } => &["sponsor", "pm"],
        // CO-10: the versioned Charter draft is recorded by the chartering role before the
        // Sponsor approves the exact version.
        CompanyCommand::DraftCharter { .. } => &["sponsor", "pm"],
        CompanyCommand::RequestCancelProject { .. }
        | CompanyCommand::ConfirmCancelProject { .. }
        | CompanyCommand::FailProject { .. }
        | CompanyCommand::ArchiveProject { .. } => &["sponsor", "closer"],
        _ => &[],
    };
    roles.iter().map(|role| (*role).to_owned()).collect()
}

impl CompanyCommand {
    pub fn policy(&self) -> CompanyCommandPolicy {
        CompanyCommandPolicy::for_command(self)
    }

    /// CP-23 mutation class used by the protected-transaction permission table.
    pub fn mutation_class(&self) -> CompanyMutationClass {
        CompanyMutationClass::of(self)
    }

    /// Registered subject/precondition entry for this command's mutation class.
    pub fn mutation_permission(&self) -> CompanyMutationPermission {
        CompanyMutationPermission::for_class(CompanyMutationClass::of(self))
    }
}

impl CompanyProof {
    pub fn human_decision(&self) -> Option<&HumanDecision> {
        self.human_decision.as_ref()
    }
}

// ---------------------------------------------------------------------------
// CP-23 protected mutation permission table
// ---------------------------------------------------------------------------

/// Stable schema for the CP-23 protected mutation permission table.
pub const COMPANY_MUTATION_PERMISSION_SCHEMA: &str = "kiana.company-mutation-permission.v1";

/// Mutation classes registered as protected control-plane transactions.
///
/// Every [`CompanyCommand`] maps to exactly one class; the two operator surfaces
/// (`memory.review` promote and `data.governance` revoke) are registered in the same table so the
/// subject/precondition matrix has one authoritative list instead of per-call-site checks.
/// Classes whose policy roles are still empty (`risk`, `incident`) are registered but deny every
/// subject until their owning card fills the matrix; an unregistered class can never execute.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyMutationClass {
    Assignment,
    PacketClaim,
    PacketChange,
    Review,
    Acceptance,
    Delivery,
    Closeout,
    Objective,
    Project,
    Budget,
    Change,
    Run,
    Risk,
    Incident,
    Artifact,
    Outcome,
    Handoff,
    MemoryPromote,
    DataRevoke,
}

impl CompanyMutationClass {
    /// Canonical registration order; the permission table covers every entry exactly once.
    pub const ALL: [Self; 19] = [
        Self::Assignment,
        Self::PacketClaim,
        Self::PacketChange,
        Self::Review,
        Self::Acceptance,
        Self::Delivery,
        Self::Closeout,
        Self::Objective,
        Self::Project,
        Self::Budget,
        Self::Change,
        Self::Run,
        Self::Risk,
        Self::Incident,
        Self::Artifact,
        Self::Outcome,
        Self::Handoff,
        Self::MemoryPromote,
        Self::DataRevoke,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Assignment => "assignment",
            Self::PacketClaim => "packet_claim",
            Self::PacketChange => "packet_change",
            Self::Review => "review",
            Self::Acceptance => "acceptance",
            Self::Delivery => "delivery",
            Self::Closeout => "closeout",
            Self::Objective => "objective",
            Self::Project => "project",
            Self::Budget => "budget",
            Self::Change => "change",
            Self::Run => "run",
            Self::Risk => "risk",
            Self::Incident => "incident",
            Self::Artifact => "artifact",
            Self::Outcome => "outcome",
            Self::Handoff => "handoff",
            Self::MemoryPromote => "memory_promote",
            Self::DataRevoke => "data_revoke",
        }
    }

    /// Capability operation that carries an operator mutation surface.
    pub fn surface(self) -> &'static str {
        match self {
            Self::MemoryPromote => "memory.review",
            Self::DataRevoke => "data.governance",
            _ => "company",
        }
    }

    pub fn is_operator_surface(self) -> bool {
        matches!(self, Self::MemoryPromote | Self::DataRevoke)
    }

    /// Register an operator mutation surface by capability operation and action.
    ///
    /// Only the two surfaces CP-23 names are registered; every other operation/action pair stays
    /// outside the table and cannot claim the operator subject.
    pub fn operator_surface(operation: &str, action: &str) -> Option<Self> {
        match (operation, action) {
            ("memory.review", "accept_proposal") => Some(Self::MemoryPromote),
            ("data.governance", "delete" | "revoke" | "expire") => Some(Self::DataRevoke),
            _ => None,
        }
    }

    /// Total mapping from a protected Company command to its mutation class.
    pub fn of(command: &CompanyCommand) -> Self {
        match command {
            CompanyCommand::Business { action, .. } => match action.as_ref() {
                CompanyBusinessAction::Closeout { .. } => Self::Closeout,
                CompanyBusinessAction::CreateOrganization { .. }
                | CompanyBusinessAction::Appoint { .. }
                | CompanyBusinessAction::RevokeAssignment { .. } => Self::Assignment,
                CompanyBusinessAction::ApproveCharter { .. } => Self::Project,
                CompanyBusinessAction::PublishPlan { .. }
                | CompanyBusinessAction::Rework { .. } => Self::PacketChange,
                CompanyBusinessAction::RegisterEvidence { .. }
                | CompanyBusinessAction::CollectEvidence { .. } => Self::Artifact,
                CompanyBusinessAction::RequestAcceptance { .. }
                | CompanyBusinessAction::DecideAcceptance { .. } => Self::Acceptance,
                CompanyBusinessAction::StartReview { .. }
                | CompanyBusinessAction::ReconcileReview { .. }
                | CompanyBusinessAction::RecordReview { .. } => Self::Review,
                CompanyBusinessAction::Pause { .. } | CompanyBusinessAction::Resume { .. } => {
                    Self::Project
                }
            },
            CompanyCommand::RegisterArtifact { .. } => Self::Artifact,
            CompanyCommand::ProposeObjective { .. }
            | CompanyCommand::DecideObjective { .. }
            | CompanyCommand::AchieveObjective { .. }
            | CompanyCommand::SubmitInitiative { .. }
            | CompanyCommand::AdvanceInitiative { .. } => Self::Objective,
            CompanyCommand::ProposeProject { .. }
            | CompanyCommand::RecordDirectIntake { .. }
            | CompanyCommand::DraftCharter { .. }
            | CompanyCommand::StartChartering { .. }
            | CompanyCommand::ApproveProject { .. }
            | CompanyCommand::RejectProject { .. }
            | CompanyCommand::CreateMilestone { .. }
            | CompanyCommand::PlanProject { .. }
            | CompanyCommand::PauseProject { .. }
            | CompanyCommand::ResumeProject { .. }
            | CompanyCommand::RequestCancelProject { .. }
            | CompanyCommand::ConfirmCancelProject { .. }
            | CompanyCommand::FailProject { .. }
            | CompanyCommand::ArchiveProject { .. } => Self::Project,
            CompanyCommand::ApprovePacket { .. } | CompanyCommand::ReworkPacket { .. } => {
                Self::PacketChange
            }
            CompanyCommand::AcknowledgeHandoff { .. } => Self::Handoff,
            CompanyCommand::ReviewPacket { .. } | CompanyCommand::RecordReview { .. } => {
                Self::Review
            }
            CompanyCommand::ConfigureBudget { .. } => Self::Budget,
            CompanyCommand::ClaimPacket { .. }
            | CompanyCommand::RenewPacketClaim { .. }
            | CompanyCommand::ReclaimPacketClaim { .. } => Self::PacketClaim,
            CompanyCommand::StartRun { .. }
            | CompanyCommand::ReconcileRun { .. }
            | CompanyCommand::RecordRunStarted { .. } => Self::Run,
            CompanyCommand::RequestAcceptance { .. } | CompanyCommand::DecideAcceptance { .. } => {
                Self::Acceptance
            }
            CompanyCommand::PrepareDelivery { .. }
            | CompanyCommand::ApproveDelivery { .. }
            | CompanyCommand::Deliver { .. }
            | CompanyCommand::ConfirmDelivery { .. }
            | CompanyCommand::MarkDeliveryUnknown { .. }
            | CompanyCommand::ReconcileDelivery { .. } => Self::Delivery,
            CompanyCommand::CloseProject { .. } => Self::Closeout,
            CompanyCommand::RecordOutcome { .. } => Self::Outcome,
            CompanyCommand::RequestChange { .. } | CompanyCommand::DecideChange { .. } => {
                Self::Change
            }
            CompanyCommand::IdentifyRisk { .. } | CompanyCommand::AdvanceRisk { .. } => Self::Risk,
            CompanyCommand::OpenIncident { .. } | CompanyCommand::AdvanceIncident { .. } => {
                Self::Incident
            }
        }
    }
}

/// Registered subjects, evidence requirements and preconditions for one mutation class.
///
/// The entry is a server-derived snapshot: callers cannot supply it, and the ControlPlane
/// revalidates it against the per-command [`CompanyCommandPolicy`] before the transition.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyMutationPermission {
    pub schema: String,
    pub class: CompanyMutationClass,
    /// Capability operation (`company`, `memory.review`, `data.governance`).
    pub surface: String,
    /// Operator action names; empty on the Company aggregate.
    pub actions: Vec<String>,
    /// Union of the per-command policy roles; empty when the class registers no subject.
    pub allowed_roles: Vec<String>,
    /// Union of the per-command actor kinds; empty when the class registers no subject.
    pub actor_kinds: Vec<DecisionActorKind>,
    /// Operator surfaces are submitted by the authenticated operator, never by a packet/cell role.
    pub operator_only: bool,
    /// At least one command in the class requires a human decision.
    pub human_decision_required: bool,
    pub assignment_required: bool,
    /// At least one command in the class requires verified evidence.
    pub evidence_required: bool,
    /// Distinct decision purposes carried by the class, in canonical order.
    pub decision_purposes: Vec<DecisionPurpose>,
    /// Stable denial/precondition codes enforced at the transaction boundary for this class.
    pub preconditions: Vec<String>,
    /// Stable code returned when an operator surface is submitted by a disallowed subject.
    pub subject_denied_code: String,
    pub permission_digest: String,
}

impl CompanyMutationPermission {
    /// Build the registered entry for one class. The data is hand-kept and cross-checked against
    /// the per-command policy by [`Self::authorize_policy`].
    pub fn for_class(class: CompanyMutationClass) -> Self {
        let roles: &[&str] = match class {
            CompanyMutationClass::Assignment | CompanyMutationClass::Budget => &["sponsor"],
            CompanyMutationClass::PacketClaim => &["builder", "closer"],
            CompanyMutationClass::PacketChange | CompanyMutationClass::Change => &["pm", "sponsor"],
            CompanyMutationClass::Review => &["reviewer"],
            CompanyMutationClass::Acceptance => &["builder", "closer", "pm", "reviewer", "sponsor"],
            CompanyMutationClass::Delivery | CompanyMutationClass::Outcome => {
                &["closer", "sponsor"]
            }
            CompanyMutationClass::Closeout => &["closer", "sponsor", "pm"],
            CompanyMutationClass::Objective => &["sponsor", "pm"],
            CompanyMutationClass::Project => &["sponsor", "pm", "closer"],
            CompanyMutationClass::Run => &["builder", "closer"],
            CompanyMutationClass::Artifact => &["builder", "reviewer", "architect", "pm", "closer"],
            CompanyMutationClass::Handoff => &["builder", "pm"],
            CompanyMutationClass::Risk
            | CompanyMutationClass::Incident
            | CompanyMutationClass::MemoryPromote
            | CompanyMutationClass::DataRevoke => &[],
        };
        let operator_only = class.is_operator_surface();
        let actor_kinds = if operator_only {
            vec![DecisionActorKind::Human, DecisionActorKind::Service]
        } else if roles.is_empty() {
            Vec::new()
        } else {
            vec![
                DecisionActorKind::Human,
                DecisionActorKind::Agent,
                DecisionActorKind::Service,
            ]
        };
        let purposes: &[DecisionPurpose] = match class {
            CompanyMutationClass::Assignment => &[DecisionPurpose::ProjectControl],
            CompanyMutationClass::PacketChange => &[
                DecisionPurpose::None,
                DecisionPurpose::PacketApproval,
                DecisionPurpose::ChangeApproval,
            ],
            CompanyMutationClass::Review => &[DecisionPurpose::None],
            CompanyMutationClass::Acceptance => {
                &[DecisionPurpose::None, DecisionPurpose::AcceptanceDecision]
            }
            CompanyMutationClass::Delivery => &[
                DecisionPurpose::None,
                DecisionPurpose::DeliveryApproval,
                DecisionPurpose::DeliveryConfirmation,
            ],
            CompanyMutationClass::Closeout => &[
                DecisionPurpose::None,
                DecisionPurpose::DeliveryApproval,
                DecisionPurpose::DeliveryConfirmation,
                DecisionPurpose::ChangeApproval,
                DecisionPurpose::ObjectiveApproval,
            ],
            CompanyMutationClass::Objective => {
                &[DecisionPurpose::None, DecisionPurpose::ObjectiveApproval]
            }
            CompanyMutationClass::Project => &[
                DecisionPurpose::None,
                DecisionPurpose::ProjectApproval,
                DecisionPurpose::ProjectControl,
            ],
            CompanyMutationClass::Budget => &[DecisionPurpose::BudgetApproval],
            CompanyMutationClass::Change => {
                &[DecisionPurpose::None, DecisionPurpose::ChangeApproval]
            }
            CompanyMutationClass::PacketClaim
            | CompanyMutationClass::Run
            | CompanyMutationClass::Risk
            | CompanyMutationClass::Incident
            | CompanyMutationClass::Artifact
            | CompanyMutationClass::Outcome
            | CompanyMutationClass::Handoff
            | CompanyMutationClass::MemoryPromote
            | CompanyMutationClass::DataRevoke => &[DecisionPurpose::None],
        };
        let preconditions: &[&str] = match class {
            CompanyMutationClass::Assignment => &[
                "business_human_decision_required",
                "business_organization_scope_denied",
                "business_assignment_invalid",
                "business_assignment_project_mismatch",
                "business_assignment_owner_or_state_invalid",
            ],
            CompanyMutationClass::PacketClaim => &[
                "company_packet_not_found",
                "company_packet_not_claimed",
                "company_claim_not_expired",
                "company_claim_session_mismatch",
                "company_claim_worker_not_stopped",
            ],
            CompanyMutationClass::PacketChange => &[
                "business_plan_baseline_stale",
                "business_rework_rejection_required",
                "business_rework_unresolved_attempt",
                "business_rework_limit_exhausted",
                "business_rework_cannot_change_baseline",
            ],
            CompanyMutationClass::Review => &[
                "business_acceptance_not_reviewable",
                "business_reviewer_assignment_inactive",
                "business_author_cannot_review",
                "business_review_evidence_unverified",
                "business_review_criterion_coverage_invalid",
                "business_review_model_result_mismatch",
            ],
            CompanyMutationClass::Acceptance => &[
                "business_acceptance_not_ready",
                "business_acceptance_baseline_stale",
                "business_independent_review_required",
                "business_acceptance_author_or_evidence_mismatch",
                "business_required_criterion_not_passed",
                "business_waiver_human_sponsor_required",
            ],
            CompanyMutationClass::Delivery => &[
                "delivery_runtime_receipt_required",
                "delivery_acceptance_required",
                "delivery_artifact_evidence_required",
                "delivery_recipient_ack_required",
                "delivery_already_exists",
            ],
            CompanyMutationClass::Closeout => &[
                "project_not_accepted",
                "closing_runtime_receipt_required",
                "closing_review_required",
                "closing_role_separation_required",
                "closing_confirmed_delivery_required",
                "closing_incident_unresolved",
            ],
            CompanyMutationClass::Objective => &[
                "objective_initial_state_invalid",
                "objective_owner_mismatch",
                "objective_organization_owner_mismatch",
                "objective_measurement_method_required",
                "objective_outcome_measurement_required",
                "initiative_information_gaps_invalid",
                "initiative_information_gaps_required",
                "initiative_conversion_requires_approval",
                "initiative_already_converted",
                "initiative_project_already_linked",
                "initiative_objective_approval_required",
            ],
            CompanyMutationClass::Project => &[
                "company_project_not_found",
                "project_initial_state_invalid",
                "project_charter_artifact_required",
                "project_sponsor_mismatch",
                "project_budget_required",
                "project_objective_not_active",
                "project_intake_provenance_required",
                "project_intake_objective_ancestry_mismatch",
                "direct_intake_reason_invalid",
                "direct_intake_objective_required",
                "direct_intake_project_not_pending",
                "direct_intake_initiative_ancestry_conflict",
                "direct_intake_conflict",
            ],
            CompanyMutationClass::Budget => &[
                "company_project_not_budgetable",
                "company_budget_scope_mismatch",
                "company_budget_already_configured",
                "project_budget_required",
            ],
            CompanyMutationClass::Change => &[
                "change_initial_state_invalid",
                "change_not_pending",
                "change_baseline_version_stale",
                "change_active_run_denied",
            ],
            CompanyMutationClass::Run => &[
                "business_packet_blocked",
                "business_assignment_required",
                "project_not_active",
                "project_other_runs_incomplete",
                "project_packets_incomplete",
            ],
            CompanyMutationClass::Risk => &[
                "risk_initial_state_invalid",
                "risk_owner_mismatch",
                "risk_incident_required",
                "risk_incident_mismatch",
                "risk_incident_unresolved",
            ],
            CompanyMutationClass::Incident => &[
                "company_incident_exists_or_invalid",
                "incident_owner_mismatch",
                "incident_not_found",
                "project_incident_mismatch",
            ],
            CompanyMutationClass::Artifact => &[
                "business_artifact_producer_unverified",
                "business_artifact_content_required",
                "business_artifact_content_mismatch",
                "business_artifact_scope_or_producer_mismatch",
                "business_run_evidence_missing",
            ],
            CompanyMutationClass::Outcome => &[
                "outcome_closed_project_required",
                "outcome_measurement_window_or_owner_invalid",
                "outcome_already_recorded",
            ],
            CompanyMutationClass::Handoff => &[
                "handoff_not_found",
                "handoff_ack_required",
                "project_frozen_packet_required",
            ],
            CompanyMutationClass::MemoryPromote => &[
                "memory_review_operator_required",
                "memory_proposal_not_found",
                "memory_proposal_source_required",
                "memory_proposal_source_invalid",
                "memory_proposal_data_revoked",
            ],
            CompanyMutationClass::DataRevoke => &[
                "governance_operator_required",
                "governance_project_required",
                "governance_actor_required",
                "data_revocation_authorization_missing",
                "data_revocation_cursor_page_limit",
            ],
        };
        let actions: &[&str] = match class {
            CompanyMutationClass::MemoryPromote => &["accept_proposal"],
            CompanyMutationClass::DataRevoke => &["delete", "revoke", "expire"],
            _ => &[],
        };
        let mut permission = Self {
            schema: COMPANY_MUTATION_PERMISSION_SCHEMA.to_owned(),
            class,
            surface: class.surface().to_owned(),
            actions: actions.iter().map(|action| (*action).to_owned()).collect(),
            allowed_roles: roles.iter().map(|role| (*role).to_owned()).collect(),
            actor_kinds,
            operator_only,
            human_decision_required: purposes.iter().any(|purpose| purpose.requires_human()),
            assignment_required: !operator_only,
            evidence_required: !matches!(
                class,
                CompanyMutationClass::Risk | CompanyMutationClass::Incident
            ),
            decision_purposes: purposes.to_vec(),
            preconditions: preconditions
                .iter()
                .map(|code| (*code).to_owned())
                .collect(),
            subject_denied_code: if operator_only {
                preconditions
                    .first()
                    .copied()
                    .unwrap_or("operator_required")
                    .to_owned()
            } else {
                "company_role_denied".to_owned()
            },
            permission_digest: String::new(),
        };
        permission.permission_digest = permission.digest();
        permission
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "class": self.class,
            "surface": self.surface,
            "actions": self.actions,
            "allowed_roles": self.allowed_roles,
            "actor_kinds": self.actor_kinds,
            "operator_only": self.operator_only,
            "human_decision_required": self.human_decision_required,
            "assignment_required": self.assignment_required,
            "evidence_required": self.evidence_required,
            "decision_purposes": self.decision_purposes,
            "preconditions": self.preconditions,
            "subject_denied_code": self.subject_denied_code,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_MUTATION_PERMISSION_SCHEMA
            || self.surface != self.class.surface()
            || self.permission_digest != self.digest()
            || self.subject_denied_code.trim().is_empty()
            || self.preconditions.is_empty()
            || self
                .preconditions
                .iter()
                .any(|code| code.trim().is_empty() || code.len() > 128)
            || self.preconditions.iter().collect::<BTreeSet<_>>().len() != self.preconditions.len()
            || self.allowed_roles.is_empty() != self.actor_kinds.is_empty()
        {
            return Err("company_mutation_permission_invalid".to_owned());
        }
        if self.operator_only {
            if self.actions.is_empty()
                || self.human_decision_required
                || self.assignment_required
                || self
                    .actor_kinds
                    .iter()
                    .any(|kind| *kind == DecisionActorKind::Agent)
            {
                return Err("company_operator_mutation_permission_invalid".to_owned());
            }
        } else if !self.actions.is_empty()
            || self.class.is_operator_surface()
            || self
                .actor_kinds
                .iter()
                .any(|kind| *kind == DecisionActorKind::Unknown)
        {
            return Err("company_mutation_permission_invalid".to_owned());
        }
        if self.allowed_roles.iter().any(|role| role.trim().is_empty())
            || self.allowed_roles.iter().collect::<BTreeSet<_>>().len() != self.allowed_roles.len()
            || self
                .allowed_roles
                .iter()
                .any(|role| !command_policy_role_known(role))
        {
            return Err("company_mutation_permission_role_invalid".to_owned());
        }
        let mut purposes = BTreeSet::new();
        if self
            .decision_purposes
            .iter()
            .any(|purpose| !purposes.insert(*purpose))
        {
            return Err("company_mutation_permission_purpose_invalid".to_owned());
        }
        if self
            .decision_purposes
            .iter()
            .any(|purpose| purpose.requires_human())
            != self.human_decision_required
        {
            return Err("company_mutation_permission_purpose_invalid".to_owned());
        }
        Ok(())
    }

    /// Revalidate the class entry against the per-command policy at the transaction boundary.
    ///
    /// A policy that asks for a subject, actor kind, purpose or evidence the class entry does not
    /// register is a drift bug and fails closed before the transition.
    pub fn authorize_policy(&self, policy: &CompanyCommandPolicy) -> Result<(), &'static str> {
        self.validate()
            .map_err(|_| "company_mutation_permission_invalid")?;
        policy
            .validate()
            .map_err(|_| "company_command_policy_invalid")?;
        if self.class.is_operator_surface()
            || policy
                .allowed_roles
                .iter()
                .any(|role| !self.allowed_roles.contains(role))
            || policy
                .actor_kinds
                .iter()
                .any(|kind| !self.actor_kinds.contains(kind))
            || !self.decision_purposes.contains(&policy.purpose)
            || (policy.evidence_required && !self.evidence_required)
        {
            return Err("company_mutation_permission_drift");
        }
        Ok(())
    }

    pub fn allows_role(&self, role: &str) -> bool {
        self.allowed_roles.iter().any(|allowed| allowed == role)
    }

    /// Operator-surface admission for a request context (Company/daemon boundary).
    pub fn authorize_operator_context(&self, context: &RequestContext) -> Result<(), String> {
        self.validate()?;
        if !self.operator_only {
            return Ok(());
        }
        let actor = context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|actor| !actor.is_empty());
        let kind = DecisionActorKind::from_context(context);
        if context.cell_id.is_some() || actor.is_none() || !self.actor_kinds.contains(&kind) {
            return Err(self.subject_denied_code.clone());
        }
        Ok(())
    }

    /// Operator-surface admission for a dispatch-level capability request.
    pub fn authorize_operator_request(
        &self,
        cell_id: Option<&crate::CellId>,
        actor_id: Option<&str>,
    ) -> Result<(), String> {
        self.validate()?;
        if !self.operator_only {
            return Ok(());
        }
        let actor = actor_id
            .map(str::trim)
            .filter(|actor| !actor.is_empty())
            .ok_or_else(|| self.subject_denied_code.clone())?;
        let kind = if actor.starts_with("service:") || actor.starts_with("system:") {
            DecisionActorKind::Service
        } else {
            DecisionActorKind::Human
        };
        if cell_id.is_some() || !self.actor_kinds.contains(&kind) {
            return Err(self.subject_denied_code.clone());
        }
        Ok(())
    }
}

/// Canonical permission table for every registered mutation class.
pub fn company_mutation_permissions() -> Vec<CompanyMutationPermission> {
    CompanyMutationClass::ALL
        .iter()
        .copied()
        .map(CompanyMutationPermission::for_class)
        .collect()
}

/// Registered operator mutation surface for a capability operation/action pair.
pub fn operator_mutation_permission(
    operation: &str,
    action: &str,
) -> Option<CompanyMutationPermission> {
    CompanyMutationClass::operator_surface(operation, action)
        .map(CompanyMutationPermission::for_class)
}

/// Roles the server-side catalog knows; the permission table cannot invent new subjects.
fn command_policy_role_known(role: &str) -> bool {
    crate::RoleSpec::lookup(role).is_some()
}
