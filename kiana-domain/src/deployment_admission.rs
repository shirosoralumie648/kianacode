//! 部署接纳（admission）、维护窗口和排空决策的数据合同，属于 `kiana-domain`。
//!
//! 【系统位置与预期数据流】
//! ```text
//! 外部协调逻辑整理健康/启动/租约/operation/维护状态
//!     -> DeploymentAdmissionInput（调用方提供的事实快照）
//!     -> 本文件 validate() 检查结构与自摘要
//!     -> DeploymentAdmissionDecision::evaluate()
//!     -> derive_decision() 按固定优先级算出状态和两个 work 开关
//!     -> DaemonHost / 上层调用者消费结果
//! ```
//!
//! `allow_new_work` 与 `allow_existing_work` 分开，是因为暂停接新任务或等待排空时，已有工作可能仍可继续。
//! 但本文件只计算一个纯决策值：不查 Health/Lease/EventLog，不读系统时钟，不暂停队列、不拒绝真实请求，也不停止
//! operation。当前还没有生产代码组装该 input 或消费结果；DaemonHost 只暴露了可调用路由。
//!
//! Serde 的 `Serialize`/`Deserialize` 用于把这些带版本的 Rust 值与外部 JSON 等格式互相转换；
//! `deny_unknown_fields` 会拒绝合同中没有声明的字段，以免调用方误以为新字段已被旧版本处理。

use crate::{json_digest, SchemaVersion, SignalStatus};
use serde::{Deserialize, Serialize};

/// `MaintenanceWindow` 序列化合同的稳定 schema 名称。
pub const MAINTENANCE_WINDOW_SCHEMA: &str = "kiana.maintenance-window.v1";
/// 准入输入序列化合同的稳定 schema 名称。
pub const DEPLOYMENT_ADMISSION_INPUT_SCHEMA: &str = "kiana.deployment-admission-input.v1";
/// 准入决策序列化合同的稳定 schema 名称。
pub const DEPLOYMENT_ADMISSION_DECISION_SCHEMA: &str = "kiana.deployment-admission-decision.v1";
/// 本文件所有 wire 值当前使用的版本；改动 wire 含义时不能只改 reducer 注释。
pub const DEPLOYMENT_ADMISSION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// window ID、reason 和 remediation 的最大 UTF-8 字节数；Rust `String::len()` 按字节计数。
const MAX_ADMISSION_TEXT: usize = 256;

/// 一段由调用方提供的维护时间范围。
///
/// 它用开始/结束 Unix 毫秒和 revision 描述窗口，并用 `window_digest` 检测字段是否被改动。
/// 本类型不保存窗口、不保证 ID 唯一，也不查询当前配置版本；“窗口有效”只表示字段自洽。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceWindow {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// wire schema 版本。
    pub version: SchemaVersion,
    /// 调用方用于识别窗口的非空文本 ID；本地不做唯一性查询。
    pub window_id: String,
    /// 窗口允许开始的 Unix 毫秒时间；判定当前是否已开始时由 input 的 `now_unix_ms` 比较。
    pub starts_at_unix_ms: u64,
    /// 窗口结束的 Unix 毫秒时间；到达此时刻即视为过期，不会自动顺延。
    pub ends_at_unix_ms: u64,
    /// 调用方提供的配置修订号；只要求非零并纳入摘要，不与配置存储做 CAS/当前版本比较。
    pub revision: u64,
    /// 除自身摘要外其余窗口字段的 SHA-256 内容摘要，不是签名或授权证明。
    pub window_digest: String,
}

impl MaintenanceWindow {
    /// 创建并校验一个维护窗口值。
    ///
    /// 【调用者】预期由维护配置的上游装配逻辑构造；当前生产源码没有该构造器的调用者，测试会直接创建窗口。
    /// 【输入】窗口 ID、开始/结束 Unix 毫秒和配置 revision；调用方必须保证时间与 revision 来自可信配置。
    /// 【输出】自动填入 schema/version/digest 并通过 `validate()` 后返回；字段非法或摘要合同不满足时返回稳定字符串错误。
    /// 【副作用/限制】只创建内存值，不写配置库、不核对授权人、不读取时钟，也不验证窗口是否是当前 revision。
    pub fn new(
        window_id: impl Into<String>,
        starts_at_unix_ms: u64,
        ends_at_unix_ms: u64,
        revision: u64,
    ) -> Result<Self, String> {
        let mut window = Self {
            schema: MAINTENANCE_WINDOW_SCHEMA.to_owned(),
            version: DEPLOYMENT_ADMISSION_VERSION,
            window_id: window_id.into(),
            starts_at_unix_ms,
            ends_at_unix_ms,
            revision,
            window_digest: String::new(),
        };
        window.window_digest = window.digest();
        window.validate()?;
        Ok(window)
    }

    /// 检查窗口的 schema、ID、时间顺序、revision 和自摘要。
    ///
    /// 【调用者】构造窗口时由 `new()` 调用，准入 input 校验时再次调用。
    /// 【失败】header 条件不满足返回 `maintenance_window_header_invalid`；摘要格式或内容不匹配返回摘要相关错误。
    /// 通过只说明这个对象内部自洽，不证明来源可信、窗口已登记或 revision 最新。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MAINTENANCE_WINDOW_SCHEMA
            || self.version != DEPLOYMENT_ADMISSION_VERSION
            || self.window_id.trim().is_empty()
            || self.window_id.len() > MAX_ADMISSION_TEXT
            || self.window_id.contains(['\0', '\r', '\n'])
            || self.starts_at_unix_ms == 0
            || self.ends_at_unix_ms <= self.starts_at_unix_ms
            || self.revision == 0
        {
            return Err("maintenance_window_header_invalid".to_owned());
        }
        valid_digest(&self.window_digest, "maintenance_window_digest")?;
        if self.window_digest != self.digest() {
            return Err("maintenance_window_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 schema/version、ID、起止时刻和 revision 计算内容摘要，不包含 `window_digest` 自身。
    ///
    /// 摘要用于发现意外字段替换；没有签名密钥，因此不能认证窗口的创建者或实际配置来源。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "window_id": self.window_id,
            "starts_at_unix_ms": self.starts_at_unix_ms,
            "ends_at_unix_ms": self.ends_at_unix_ms,
            "revision": self.revision,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 准入 reducer 生成的分类状态；是否允许新/已有工作仍要看 decision 上两个布尔字段。
pub enum DeploymentAdmissionStatus {
    /// 健康、启动、租约和冲突条件均通过，且没有暂停/维护/排空状态。
    Ready,
    /// intake 被暂停；当前合同禁止新工作但允许已有工作继续。
    Paused,
    /// 排空截止时间尚未到；禁止新工作，允许已有工作完成或由上层协调。
    Draining,
    /// 当前时间位于维护窗口内；禁止新工作，允许已有工作继续。
    Maintenance,
    /// 某个明确前置条件未满足；具体能否继续已有工作由 `allow_existing_work` 表示。
    Blocked,
    /// 事实未知、租约不匹配或排空已过期；两个 work 开关都关闭，需先 reconciliation。
    Unknown,
}

/// 计算准入决策时使用的一组带版本、可摘要的输入事实。
///
/// 各字段由调用方汇总，不会在 Domain/Core/Daemon helper 中被回查。`input_digest` 只绑定这些供给值；它不证明
/// 健康报告、启动报告、租约、operation、迁移、备份、时钟或 source cursor 来自真实或最新事实源。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentAdmissionInput {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// 输入合同版本。
    pub version: SchemaVersion,
    /// 上游健康报告摘要引用；这里只检查 SHA-256 文本格式并纳入 input digest，不读取对应报告。
    pub health_digest: String,
    /// 上游提供的健康分类；只有 `Ok` 可进入后续准入分支，`Unknown` 产生 Unknown，Error/Degraded 产生 Blocked。
    pub health_status: SignalStatus,
    /// 上游对启动是否 ready 的布尔声明；本 reducer 不运行启动检查。
    pub startup_ready: bool,
    /// 当前维护窗口快照；存在时会按 input 的 now 与窗口边界决定维护状态。
    pub maintenance_window: Option<MaintenanceWindow>,
    /// 上游提供的当前 Unix 毫秒时间；本文件不读取系统时钟或检查时钟是否可信。
    pub now_unix_ms: u64,
    /// 上游是否已暂停 intake；这里只影响决策值，不会真正暂停队列。
    pub intake_paused: bool,
    /// 上游是否处于排空阶段；为 true 时必须同时提供非零 drain deadline。
    pub drain_active: bool,
    /// 排空的绝对 Unix 毫秒截止时刻；未启用 drain 时必须为 None。
    pub drain_deadline_unix_ms: Option<u64>,
    /// 上游是否报告迁移进行中；为 true 时当前顺序会先于 drain/window/pause 阻断准入。
    pub migration_active: bool,
    /// 上游是否报告备份进行中；与迁移相同，会阻断新旧两类工作。
    pub backup_active: bool,
    /// 上游关于当前 lease 是否有效的布尔声明；本 reducer 不加载或重新校验 OperationLease。
    pub lease_valid: bool,
    /// 当前 authority epoch；只要求非零，并与随 input 提供的 lease epoch 比较。
    pub authority_epoch: u64,
    /// 当前 data epoch；只要求非零，并与随 input 提供的 lease epoch 比较。
    pub data_epoch: u64,
    /// input 所带 lease 的 authority epoch；必须与当前 authority epoch 相等。
    pub lease_authority_epoch: u64,
    /// input 所带 lease 的 data epoch；必须与当前 data epoch 相等。
    pub lease_data_epoch: u64,
    /// 上游是否无法确定 operation 状态；false 不代表没有活跃 operation，本 DTO 没有活跃数量/状态字段。
    pub operation_unknown: bool,
    /// 上游事实对应的 source cursor；这里仅要求非零并纳入摘要，不用它比较新鲜度或决定状态。
    pub source_cursor: u64,
    /// 除自身摘要外所有输入字段的 SHA-256 内容摘要，不是对字段来源的签名。
    pub input_digest: String,
}

impl DeploymentAdmissionInput {
    /// 检查 input 的 wire header、基本数值、drain/deadline 组合、嵌套窗口和自摘要。
    ///
    /// 【调用者】Domain `DeploymentAdmissionDecision::evaluate()` 在派生状态前调用；Core/Daemon 路由通过它间接校验。
    /// 【输入/输出】借用调用方组装的完整快照；合法返回 `Ok(())`，不一致返回稳定 `String` 错误。
    /// 【重要边界】只验值对象内部关系，不验证 `health_digest` 对应的实际报告、lease 当前状态、cursor 连续性、
    /// `now_unix_ms` 的时钟来源或 maintenance revision 的新鲜度。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_ADMISSION_INPUT_SCHEMA
            || self.version != DEPLOYMENT_ADMISSION_VERSION
            || self.now_unix_ms == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.lease_authority_epoch == 0
            || self.lease_data_epoch == 0
            || self.source_cursor == 0
            || (self.drain_active
                && self
                    .drain_deadline_unix_ms
                    .is_none_or(|deadline| deadline == 0))
            || (!self.drain_active && self.drain_deadline_unix_ms.is_some())
        {
            return Err("deployment_admission_input_header_invalid".to_owned());
        }
        valid_digest(&self.health_digest, "deployment_admission_health_digest")?;
        if let Some(window) = &self.maintenance_window {
            window.validate()?;
        }
        valid_digest(&self.input_digest, "deployment_admission_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("deployment_admission_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对全部准入输入计算摘要，包括 caller supplied 的状态标志、时间、窗口、epoch 和 cursor。
    ///
    /// 此处把各字段绑定在同一 DTO 上，便于后续发现内容变化；调用方修改任意字段并重算摘要仍可通过，
    /// 因此它不是授权 token、签名、持久化记录或事实来源证明。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "health_digest": self.health_digest,
            "health_status": self.health_status,
            "startup_ready": self.startup_ready,
            "maintenance_window": self.maintenance_window,
            "now_unix_ms": self.now_unix_ms,
            "intake_paused": self.intake_paused,
            "drain_active": self.drain_active,
            "drain_deadline_unix_ms": self.drain_deadline_unix_ms,
            "migration_active": self.migration_active,
            "backup_active": self.backup_active,
            "lease_valid": self.lease_valid,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "lease_authority_epoch": self.lease_authority_epoch,
            "lease_data_epoch": self.lease_data_epoch,
            "operation_unknown": self.operation_unknown,
            "source_cursor": self.source_cursor,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 对一份 `DeploymentAdmissionInput` 计算出的只读决策。
///
/// `status` 是便于解释的分类，真正区分新工作和已有工作的字段是 `allow_new_work` 与 `allow_existing_work`。
/// 正常拒绝会作为 `Ok(decision)` 返回，并非 `Err`；上层只有实际消费这些字段，才可能把决定接到执行门槛上。
pub struct DeploymentAdmissionDecision {
    /// 固定 wire schema 名称。
    pub schema: String,
    /// 决策合同版本。
    pub version: SchemaVersion,
    /// 产生本决定的 input 摘要，用于将 decision 与同一组供给事实绑定。
    pub input_digest: String,
    /// 按 reducer 优先级生成的解释性状态分类。
    pub status: DeploymentAdmissionStatus,
    /// 是否可接收新的工作；Ready 为 true，其余当前状态均为 false。
    pub allow_new_work: bool,
    /// 是否允许已有工作继续；取决于具体分支，不能仅从 status 名称推断。
    pub allow_existing_work: bool,
    /// 稳定机器可读原因码；不是面向最终用户的完整解释。
    pub reason: String,
    /// 上游处理建议的稳定标识；本 Domain 不执行 remediation。
    pub remediation: String,
    /// 除自身摘要外所有决策字段的 SHA-256 内容摘要，不是签名或真实状态证明。
    pub decision_digest: String,
}

impl DeploymentAdmissionDecision {
    /// 校验 input 并按固定优先级计算准入决策。
    ///
    /// 【调用者】Core `evaluate_admission()` 委托本方法，DaemonHost 的公开路由再通过 Core 到达这里；
    /// 目前仓库生产源码没有调用该 DaemonHost 方法的实际消费者。
    /// 【输入】调用者汇总的健康、启动、维护、暂停/排空、migration/backup、lease epoch、operation 和时间快照。
    /// 【输出】input 合同有效时返回一份带状态、new/existing work flags、reason/remediation 和摘要的 decision；
    /// 正常的 Blocked/Unknown/Paused 等状态是 `Ok` 值。只有 input 格式或摘要不合法才返回 `Err(String)`。
    /// 【副作用】只进行确定性计算；不暂停 intake、不停止 operation、不获取 lease、不写 EventLog，也不重试。
    pub fn evaluate(input: &DeploymentAdmissionInput) -> Result<Self, String> {
        input.validate()?;
        let decision = build_decision(input);
        decision.validate_against(input)?;
        Ok(decision)
    }

    /// 只检查 decision 自身的 schema、文本长度和摘要外形/一致性。
    ///
    /// 单独通过不保证 status、两个 allow flag、reason 或 remediation 是正确计算所得；需要关联 input 时必须调用
    /// `validate_against()` 重新派生并逐字段比较。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_ADMISSION_DECISION_SCHEMA
            || self.version != DEPLOYMENT_ADMISSION_VERSION
            || self.reason.trim().is_empty()
            || self.remediation.trim().is_empty()
            || self.reason.len() > MAX_ADMISSION_TEXT
            || self.remediation.len() > MAX_ADMISSION_TEXT
        {
            return Err("deployment_admission_decision_header_invalid".to_owned());
        }
        valid_digest(
            &self.input_digest,
            "deployment_admission_decision_input_digest",
        )?;
        valid_digest(
            &self.decision_digest,
            "deployment_admission_decision_digest",
        )?;
        if self.decision_digest != self.digest() {
            return Err("deployment_admission_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 重新根据给定 input 计算期望 decision，并要求当前对象与其完全一致。
    ///
    /// 【调用者】可供接收/解码 decision 的上层做关联校验；当前只有 reducer 自己在返回前调用，没有生产调用者。
    /// 【失败】先校验 input 和 decision 本身，再逐字段比较重算结果；不匹配时返回
    /// `deployment_admission_decision_binding_mismatch`。
    /// 【限制】比较的仍是同一份调用者提供的 input，不会补做 health/lease/clock/source cursor 的可信度验证。
    pub fn validate_against(&self, input: &DeploymentAdmissionInput) -> Result<(), String> {
        input.validate()?;
        self.validate()?;
        let expected = build_decision(input);
        if self != &expected {
            return Err("deployment_admission_decision_binding_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对 decision 的 schema/version、input digest、状态、允许标志和原因文本计算内容摘要。
    ///
    /// 不包含 `decision_digest` 自身；摘要不是签名，且不替代 `validate_against()` 的重算绑定检查。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "input_digest": self.input_digest,
            "status": self.status,
            "allow_new_work": self.allow_new_work,
            "allow_existing_work": self.allow_existing_work,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

/// 将 reducer 得出的字段装入 decision，再计算 decision digest。
///
/// 只由 `DeploymentAdmissionDecision::evaluate()` 和 `validate_against()` 的重算路径调用；函数本身不做 input 校验，
/// 所以调用前必须已通过 `input.validate()`。
fn build_decision(input: &DeploymentAdmissionInput) -> DeploymentAdmissionDecision {
    let (status, allow_new_work, allow_existing_work, reason, remediation) = derive_decision(input);
    let mut decision = DeploymentAdmissionDecision {
        schema: DEPLOYMENT_ADMISSION_DECISION_SCHEMA.to_owned(),
        version: DEPLOYMENT_ADMISSION_VERSION,
        input_digest: input.input_digest.clone(),
        status,
        allow_new_work,
        allow_existing_work,
        reason,
        remediation,
        decision_digest: String::new(),
    };
    decision.decision_digest = decision.digest();
    decision
}

/// 按拒绝优先级把已校验的 caller supplied 快照折叠成状态、两个工作许可标志和原因。
///
/// 顺序是合同的一部分：先 lease/epoch 和未知事实，再健康/启动与 migration/backup，再 drain、maintenance window、
/// pause，最后 Ready。若把后面的分支提前，多个条件同时成立时状态和是否允许已有工作都会改变。
/// 此函数只派生值，不实施任何准入或排空动作。
fn derive_decision(
    input: &DeploymentAdmissionInput,
) -> (DeploymentAdmissionStatus, bool, bool, String, String) {
    // 没有有效 lease 声明或 epoch 不一致时，无法确认调用者是否仍有当前写权限；因此连已有工作也暂不允许。
    if !input.lease_valid
        || input.authority_epoch != input.lease_authority_epoch
        || input.data_epoch != input.lease_data_epoch
    {
        return (
            DeploymentAdmissionStatus::Unknown,
            false,
            false,
            "admission_lease_conflict".to_owned(),
            "reacquire_current_lease_before_admission".to_owned(),
        );
    }
    // operation/health 状态未知时保留不确定性，不能把它降格成“健康但暂时不可用”。
    if input.operation_unknown || input.health_status == SignalStatus::Unknown {
        return (
            DeploymentAdmissionStatus::Unknown,
            false,
            false,
            "admission_result_unknown".to_owned(),
            "reconcile_unknown_facts_before_admission".to_owned(),
        );
    }
    // 仅 SignalStatus::Ok 且 startup_ready=true 才进入下面的维护/排空策略；错误、降级或启动未就绪均全关。
    if input.health_status != SignalStatus::Ok || !input.startup_ready {
        return (
            DeploymentAdmissionStatus::Blocked,
            false,
            false,
            "admission_health_not_ready".to_owned(),
            "complete_startup_and_health_preflight".to_owned(),
        );
    }
    // migration 或 backup 与 ready admission 冲突；当前合同对新工作和已有工作都给出 false。
    if input.migration_active || input.backup_active {
        return (
            DeploymentAdmissionStatus::Blocked,
            false,
            false,
            "admission_maintenance_conflict".to_owned(),
            "finish_migration_or_backup_before_ready".to_owned(),
        );
    }
    // Drain 优先于 maintenance window 和 pause：截止已到时结果未知并关闭两类工作；未到则只拒绝新工作，
    // 让上层有机会等待已有工作排空。deadline 的可信度完全来自调用方输入。
    if input.drain_active {
        if input
            .drain_deadline_unix_ms
            .is_some_and(|deadline| input.now_unix_ms >= deadline)
        {
            return (
                DeploymentAdmissionStatus::Unknown,
                false,
                false,
                "drain_deadline_expired".to_owned(),
                "reconcile_inflight_work_after_drain_deadline".to_owned(),
            );
        }
        return (
            DeploymentAdmissionStatus::Draining,
            false,
            true,
            "draining_no_new_admission".to_owned(),
            "wait_for_drain_or_reconcile_inflight_work".to_owned(),
        );
    }
    // 有窗口对象时，先判断未开始/过期，再判断 active。过期窗口不会自动延长；window 分支优先于 intake_paused。
    if let Some(window) = &input.maintenance_window {
        if input.now_unix_ms < window.starts_at_unix_ms {
            return (
                DeploymentAdmissionStatus::Blocked,
                false,
                true,
                "maintenance_window_not_started".to_owned(),
                "wait_for_explicit_window_start".to_owned(),
            );
        }
        if input.now_unix_ms >= window.ends_at_unix_ms {
            return (
                DeploymentAdmissionStatus::Blocked,
                false,
                false,
                "maintenance_window_expired".to_owned(),
                "create_a_new_explicit_maintenance_window".to_owned(),
            );
        }
        return (
            DeploymentAdmissionStatus::Maintenance,
            false,
            true,
            "maintenance_window_active".to_owned(),
            "close_maintenance_window_before_new_admission".to_owned(),
        );
    }
    // 只有没有更高优先级的 lease/健康/冲突/drain/window 条件时，单独的 pause 才给出“拒新、允旧”。
    if input.intake_paused {
        return (
            DeploymentAdmissionStatus::Paused,
            false,
            true,
            "intake_paused".to_owned(),
            "resume_intake_with_explicit_control_plane_action".to_owned(),
        );
    }
    // 全部显式前置条件都通过且没有维护控制标志时，纯 reducer 给出 Ready；这仍不是实际队列已开放的证明。
    (
        DeploymentAdmissionStatus::Ready,
        true,
        true,
        "ok".to_owned(),
        "none".to_owned(),
    )
}

/// 检查 `sha256:` 加 64 个十六进制字符的外形，不回读或重算被引用对象。
///
/// 错误名带入调用方给出的字段前缀，供上层定位是哪一个摘要形状不合约；它不认证摘要来自可信生产者。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
