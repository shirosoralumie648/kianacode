//! Kiana ControlPlane 使用的稳定领域契约。
//!
//! 本 crate 位于依赖图底层，只定义 ID、角色/部门、WorkPacket、能力请求、审批、事件和
//! 生命周期等值对象，不依赖 daemon、query 或具体执行器。结构体的序列化形状是跨模块
//! 的契约，但“存在一个类型”不等于运行时已经强制：真正的授权顺序、CAS、lease 和持久
//! 证据仍由 core 与 ports 的实现负责。
//!
//! 所有路径、能力和状态 helper 都采用收紧/拒绝优先的语义。调用方应把 `None`、拒绝和
//! `ResultUnknown` 与“没有副作用”严格区分，并以 EventLog/Receipt 作为事实来源，不把
//! transcript、UI 投影或模型自述当作状态权威。

mod capabilities;
mod ids;
mod paths;
mod roles;
mod states;
mod symposiums;
#[cfg(test)]
mod tests;
mod work_packets;

pub use capabilities::*;
pub use ids::*;
pub use paths::*;
pub use roles::*;
pub use states::*;
pub use symposiums::*;
pub use work_packets::*;

pub const APPROVAL_CHALLENGE_SCHEMA: &str = "kiana.approval-challenge.v1";
pub const ROLE_BUILDER: &str = "builder";
pub const ROLE_PM: &str = "pm";
pub const ROLE_ARCHITECT: &str = "architect";
pub const ROLE_REVIEWER: &str = "reviewer";
pub const ROLE_SPONSOR: &str = "sponsor";
pub const ROLE_CLOSER: &str = "closer";
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
