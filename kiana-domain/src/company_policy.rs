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
        CompanyCommand::StartRun { .. } | CompanyCommand::RequestAcceptance { .. } => {
            &["builder", "closer"]
        }
        CompanyCommand::ReviewPacket { .. } | CompanyCommand::RecordReview { .. } => &["reviewer"],
        CompanyCommand::DecideAcceptance { .. } => &["reviewer", "sponsor"],
        CompanyCommand::PrepareDelivery { .. }
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
        | CompanyCommand::RequestChange { .. } => &["sponsor", "pm"],
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
}

impl CompanyProof {
    pub fn human_decision(&self) -> Option<&HumanDecision> {
        self.human_decision.as_ref()
    }
}
