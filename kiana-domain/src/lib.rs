//! Kiana ControlPlane 使用的稳定领域契约。
//!
//! 本 crate 位于依赖图底层，定义 ID、角色/部门、WorkPacket、能力请求、审批、事件和
//! 生命周期等值对象，以及跨层共享的文本脱敏原语，不依赖 daemon、query 或具体执行器。
//! 结构体的序列化形状是跨模块的契约，但“存在一个类型”不等于运行时已经强制：真正的
//! 授权顺序、CAS、lease 和持久证据仍由 core 与 ports 的实现负责。
//!
//! 所有路径、能力和状态 helper 都采用收紧/拒绝优先的语义。调用方应把 `None`、拒绝和
//! `ResultUnknown` 与“没有副作用”严格区分，并以 EventLog/Receipt 作为事实来源，不把
//! transcript、UI 投影或模型自述当作状态权威。

mod adapter_result;
mod approval_journal;
mod artifact_contracts;
mod assignment;
mod audit;
mod authority;
mod budget_contracts;
mod cancellation;
mod capabilities;
mod communication;
pub mod company;
mod company_policy;
mod company_receipts;
mod company_replay;
mod company_scope;
mod connectors;
mod context_scope;
mod contracts;
mod errors;
mod eval;
mod event_contracts;
mod execution_identity;
mod execution_scope;
mod extension_catalog;
mod extension_contracts;
mod extensions;
mod fault;
mod fencing;
mod governance_gate;
mod grant_authority;
mod identity;
mod identity_contracts;
mod ids;
mod live_handoff;
mod memory;
mod memory_distillation;
mod memory_mutation;
mod memory_proposals;
mod notification_events;
mod notifications;
mod parity;
mod paths;
mod performance;
mod platform;
mod projection_contracts;
mod prompts;
mod provider_config;
mod quality;
mod receipt_aggregation;
mod receipt_contracts;
mod recovery_resources;
mod redaction;
mod resource_leases;
mod roles;
mod scope;
mod security_contracts;
mod security_reasons;
mod session_contracts;
mod states;
mod storage;
mod storage_health;
mod storage_schema;
mod swarm_graph;
mod swarm_identity;
mod swarm_reducer;
mod symposiums;
#[cfg(test)]
mod tests;
mod trust_snapshots;
mod usage;
mod work_packets;

pub use adapter_result::*;
pub use approval_journal::*;
pub use artifact_contracts::*;
pub use assignment::*;
pub use audit::*;
pub use authority::*;
pub use budget_contracts::*;
pub use cancellation::*;
pub use capabilities::*;
pub use communication::*;
pub use company::*;
pub use company_policy::*;
pub use company_receipts::*;
pub use company_replay::*;
pub use company_scope::*;
pub use connectors::*;
pub use context_scope::*;
pub use contracts::*;
pub use errors::*;
pub use eval::*;
pub use event_contracts::*;
pub use execution_identity::*;
pub use execution_scope::*;
pub use extension_catalog::*;
pub use extension_contracts::*;
pub use extensions::*;
pub use fault::*;
pub use fencing::*;
pub use governance_gate::*;
pub use grant_authority::*;
pub use identity::*;
pub use identity_contracts::*;
pub use ids::*;
pub use live_handoff::*;
pub use memory::*;
pub use memory_distillation::*;
pub use memory_mutation::*;
pub use memory_proposals::*;
pub use notification_events::*;
pub use notifications::*;
pub use parity::*;
pub use paths::*;
pub use performance::*;
pub use platform::*;
pub use projection_contracts::*;
pub use prompts::*;
pub use provider_config::*;
pub use quality::*;
pub use receipt_aggregation::*;
pub use receipt_contracts::*;
pub use recovery_resources::*;
pub use redaction::*;
pub use resource_leases::*;
pub use roles::*;
pub use scope::*;
pub use security_contracts::*;
pub use security_reasons::*;
pub use session_contracts::*;
pub use states::*;
pub use storage::*;
pub use storage_health::*;
pub use storage_schema::*;
pub use swarm_graph::*;
pub use swarm_identity::*;
pub use swarm_reducer::*;
pub use symposiums::*;
pub use trust_snapshots::*;
pub use usage::*;
pub use work_packets::*;

pub const APPROVAL_CHALLENGE_SCHEMA: &str = "kiana.approval-challenge.v1";
pub const ROLE_BUILDER: &str = "builder";
pub const ROLE_PM: &str = "pm";
pub const ROLE_ARCHITECT: &str = "architect";
pub const ROLE_REVIEWER: &str = "reviewer";
pub const ROLE_SPONSOR: &str = "sponsor";
pub const ROLE_CLOSER: &str = "closer";
pub const ROLE_ANALYST: &str = "analyst";
pub const ROLE_QA: &str = "qa";
pub const ROLE_LIBRARIAN: &str = "librarian";
pub const ROLE_SPEC_SCHEMA: &str = "kiana.role-spec.v1";
pub const ROLE_CATALOG_SCHEMA: &str = "kiana.role-catalog.v1";
pub const DEPARTMENT_SPEC_SCHEMA: &str = "kiana.department-spec.v1";
pub const DEPARTMENT_CATALOG_SCHEMA: &str = "kiana.department-catalog.v1";
pub const ROLE_INPUT_SCHEMA_PREFIX: &str = "kiana.company-role-input";
pub const ROLE_OUTPUT_SCHEMA_PREFIX: &str = "kiana.company-role-output";
pub const DEPARTMENT_EXECUTING: &str = "executing";
pub const DEPARTMENT_PLANNING: &str = "planning";
pub const DEPARTMENT_MONITORING: &str = "monitoring";
pub const DEPARTMENT_INITIATING: &str = "initiating";
pub const DEPARTMENT_CLOSING: &str = "closing";
pub const ROLE_SANDBOX_READ_ONLY: &str = "read-only";
pub const ROLE_SANDBOX_WORKSPACE_WRITE: &str = "workspace-write";
pub const PLANNING_PATH_CHARTER: &str = "charter";
pub const PLANNING_PATH_PLAN: &str = "plan";
pub const PLANNING_PATH_PACKET: &str = "packet";
pub const CLOSING_PATH_LESSONS: &str = "lessons";
pub const EXECUTING_PATH_RECEIPT: &str = "receipt";
pub const WORK_PACKET_SCHEMA: &str = "kiana.work-packet.v1";
pub const DECISION_RECORD_SCHEMA: &str = "kiana.decision-record.v1";
pub const SYMPOSIUM_SCHEMA: &str = "kiana.symposium.v1";
pub const SYMPOSIUM_RESULT_SCHEMA: &str = "kiana.symposium-result.v1";
pub const SYMPOSIUM_TYPE_DECISION: &str = "decision";
pub const SYMPOSIUM_STATUS_CLOSED: &str = "closed";
pub const SYMPOSIUM_STATUS_SKIPPED: &str = "skipped";
pub const DECISION_RECORD_PATH: &str = "plan/DECISION.json";
pub const INITIATING_DECISION_PATH: &str = "charter/DECISION.json";
pub const EXECUTING_DECISION_PATH: &str = "receipt/DECISION.json";
pub const MONITORING_DECISION_PATH: &str = "gate/DECISION.json";
pub const CLOSING_DECISION_PATH: &str = "lessons/DECISION.json";
pub const CLOSING_RECEIPT_PATH: &str = "lessons/CLOSING.json";
pub const WORK_PACKET_PATH: &str = "packet/TASK.json";
pub const REVIEW_PACKET_SCHEMA: &str = "kiana.review-packet.v1";
pub const REVIEW_RESULT_SCHEMA: &str = "kiana.review-result.v1";
pub const REVIEW_PACKET_PATH: &str = "gate/REVIEW.json";
pub const MERGE_RECEIPT_PATH: &str = "gate/MERGE.json";
pub const MONITORING_PATH_GATE: &str = "gate";
pub const MEMORY_LAYER_COMPANY: &str = "company";
pub const MEMORY_LAYER_DEPARTMENT: &str = "department";
pub const MEMORY_LAYER_ROLE: &str = "role";
pub const MEMORY_LAYER_PROJECT: &str = "project";
pub const MEMORY_LAYER_USER: &str = "user";
pub const MEMORY_LAYER_INSTANCE_SCRATCH: &str = "instance-scratch";
pub const MEMORY_COLLECTION_USER_PRIVATE: &str = "user-private";
pub const MEMORY_COLLECTION_USER_PREFS: &str = "user:prefs";
pub const MEMORY_COLLECTION_PLANNING_UNRELEASED: &str = "planning:unreleased-debate";
pub const MEMORY_SEARCH_SCHEMA: &str = "kiana.memory-search.v1";
pub const MEMORY_WRITE_SCHEMA: &str = "kiana.memory-write.v1";
pub const MEMORY_RECORD_SCHEMA: &str = "kiana.memory-record.v1";
pub const AGENT_TEMPLATE_SCHEMA: &str = "kiana.agent-template.v1";
pub const CELL_SCHEMA: &str = "kiana.cell.v1";
pub const SPAWN_PLAN_SCHEMA: &str = "kiana.spawn-plan.v1";
pub const BUDGET_LEASE_SCHEMA: &str = "kiana.budget-lease.v1";
pub const CAPABILITY_GRANT_SCHEMA: &str = "kiana.capability-grant.v1";
pub const SUPERVISION_LEASE_SCHEMA: &str = "kiana.supervision-lease.v1";
pub const DELEGATION_PACKET_SCHEMA: &str = "kiana.delegation-packet.v1";
pub const MERGE_RECEIPT_SCHEMA: &str = "kiana.merge-receipt.v1";
pub const CLOSING_RECEIPT_SCHEMA: &str = "kiana.closing-receipt.v1";
pub const SPAWN_RESULT_SCHEMA: &str = "kiana.spawn-result.v1";
pub const RETIREMENT_RECORD_SCHEMA: &str = "kiana.retirement-record.v1";
pub const MEMORY_LAYERS: [&str; 6] = [
    MEMORY_LAYER_COMPANY,
    MEMORY_LAYER_DEPARTMENT,
    MEMORY_LAYER_ROLE,
    MEMORY_LAYER_PROJECT,
    MEMORY_LAYER_USER,
    MEMORY_LAYER_INSTANCE_SCRATCH,
];

mod tool_authority;
mod tool_catalog;
pub use tool_authority::*;
pub use tool_catalog::*;
mod governance;
pub use governance::*;
mod packet_graph;
pub use packet_graph::*;

mod automation;
pub use automation::*;

mod swarm;
pub use swarm::*;

mod handoff;
pub use handoff::*;

mod dispatch;
mod effect_observation;
pub use dispatch::*;
pub use effect_observation::*;

mod journal;
pub use journal::*;

mod actions;
pub use actions::*;

mod company_business;
pub use company_business::*;

mod model;
mod model_catalog;
pub use model::*;
pub use model_catalog::*;
mod observability;
pub use observability::*;
mod correlation;
pub use correlation::*;

mod company_closeout;
pub use company_closeout::*;
