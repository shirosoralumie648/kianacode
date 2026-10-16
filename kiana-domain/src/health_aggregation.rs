//! DEP-11 typed-fact health aggregation contract and deterministic reducer.
//!
//! 【系统位置与真实调用链】
//! ```text
//! 外部调用方收集启动 / EventLog / projection / lease / Provider / operation typed facts
//!     -> DaemonHost::{deployment_health, drain_health, maintenance_health}
//!     -> kiana-daemon::health_aggregation::{evaluate, evaluate_probe}
//!     -> kiana-core::aggregate_health()
//!     -> HealthAggregationReport::evaluate()
//!     -> build_report() / status_for()
//!     -> HealthAggregationReport + HealthSnapshot
//! ```
//!
//! 本文件属于 `kiana-domain`，负责检查输入结构、自摘要与状态之间的合同，并纯函数式地
//! 生成报告。它不会收集上面那些事实，不读取 EventStore/启动报告/租约记录，不探测
//! Provider，不调用 Broker，不持久化心跳，也不真的授权、拒绝或暂停 admission。
//! `admission_allowed` 只是报告对 readiness 结果的派生字段。
//!
//! 【与 OA-11 EventLog 健康投影的区别】
//! `kiana-core::health::project_health_snapshot()` 从 EventStore 全量事件和能力声明重建
//! 健康视图；这里的 `HealthAggregationInput` 则由调用方提供已收集的 typed facts，再复用
//! `HealthSnapshot` 作为输出格式。不要把两条路径的输入来源或证据等级混在一起。
//!
//! 【重要信任边界】
//! digest 只能检查本对象序列化字段是否一致，不能证明调用方提供的事实真实或来自可信
//! adapter。尤其 `startup_report_digest` 与 `component_evidence_digest` 只做摘要格式检查
//! 并参与 `input_digest`；本模块不会读取对应报告或组件记录来核对它们。
//!
//! 当前 `DaemonHost` 暴露了该合同的同步方法，但 workspace 内尚未找到它们的生产调用者；
//! 现有 CI fixture 直接构造本输入验证状态矩阵。该状态是 source-level 合同，不代表 CLI、
//! UI、HTTP 或统一 admission 已接入。

use crate::{
    json_digest, ComponentHealth, ComponentHealthState, EventId, HealthProbeKind, HealthSnapshot,
    OperationPhase, OperationState, SchemaVersion, SignalStatus, StartupCoordinatorStatus,
    MAX_HEALTH_LIMITATIONS, MAX_SOURCE_EVENT_IDS,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const HEALTH_AGGREGATION_SCHEMA: &str = "kiana.health-aggregation.v1";
pub const HEALTH_AGGREGATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_HEALTH_COMPONENT: usize = 128;

/// 调用方收集的一组部署健康输入事实。
///
/// 这些字段是上游适配层提交给 Domain reducer 的 typed snapshot，不由本结构或其
/// `validate()` 方法从 EventStore、StartupCoordinator、Provider、lease 服务或 operation
/// journal 中现场查询。调用方负责确保每个布尔值、epoch 和引用摘要有正确来源；本类型
/// 主要检查格式、非零/唯一约束、operation state/phase 一致性和自身 digest。
///
/// Serde 的 `Serialize` 把 Rust 值转换为 JSON 等外部格式，`Deserialize` 负责读回；
/// `deny_unknown_fields` 会拒绝未识别字段，避免不同版本间静默忽略新旧字段。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthAggregationInput {
    /// 固定 schema 名称，用于区分此合同与其他健康快照。
    pub schema: String,
    /// 该 schema 的版本；当前只接受 `HEALTH_AGGREGATION_VERSION`。
    pub version: SchemaVersion,
    /// 本次计算的问题类型：Startup、Readiness、Liveness、Drain 或 Maintenance。
    pub probe: HealthProbeKind,
    /// 总体快照代表的组件名称，例如 `control_plane`；不是某个 Provider 的名字。
    pub component: String,
    /// 本组输入证据覆盖到的源事件游标；必须大于零。
    pub source_cursor: u64,
    /// 绑定到源游标的事件 ID 引用；必须非空、唯一且不超过 Domain 上限。
    pub source_event_ids: Vec<EventId>,
    /// 生成这组 typed facts 时记录的 Unix 毫秒时间；不是 timeout 或 lease TTL。
    pub observed_at_ms: u64,
    /// 调用方读取的 authority epoch，用于与 lease 的 authority epoch 比较。
    pub authority_epoch: u64,
    /// 调用方读取的数据 epoch，用于与 lease 的 data epoch 比较。
    pub data_epoch: u64,
    /// 调用方报告的 generation 代数。当前只要求非零并纳入 digest；本 reducer 不会将它
    /// 与另一份 generation 记录比较，因为输入和 lease 字段中没有对应的 lease generation。
    pub generation: u64,
    /// StartupCoordinator 的 Ready/Blocked/Unknown 结果；Readiness 还要求此值为 Ready。
    pub startup_status: StartupCoordinatorStatus,
    /// 调用方声称对应启动报告的摘要；本 reducer 仅验证 SHA-256 字符串格式，不读取报告。
    pub startup_report_digest: String,
    /// EventStore 是否被调用方判定为可用；`false` 会使总体状态至少为 Degraded。
    pub eventlog_ready: bool,
    /// 投影已处理到的游标；落后或超前源游标都会阻止 Readiness。
    pub projection_cursor: u64,
    /// 调用方观测到的未知副作用数量；大于零时总体状态优先成为 Unknown。
    pub unknown_count: u64,
    /// 对应 operation lease 当前是否仍有效；`false` 被视为租约冲突。
    pub lease_active: bool,
    /// lease 所绑定的 authority epoch，必须与 `authority_epoch` 相同。
    pub lease_authority_epoch: u64,
    /// lease 所绑定的数据 epoch，必须与 `data_epoch` 相同。
    pub lease_data_epoch: u64,
    /// 调用方报告 lease fencing 是否有效；`false` 表示本 reducer 不能确认当前 owner
    /// 仍受有效 fence 约束。
    pub lease_fence_valid: bool,
    /// Provider 的健康结论；只有 `provider_evidence_verified=true` 时才允许作为可信健康值。
    pub provider_healthy: bool,
    /// 调用方是否验证过 Provider 健康证据。未验证时 Provider 组件为 Unknown，且 Readiness
    /// 不能通过；本 reducer 不负责完成这项验证。
    pub provider_evidence_verified: bool,
    /// 是否正在 drain（停止接收新工作的排空阶段）；Readiness 因此必须关闭。
    pub drain_active: bool,
    /// 是否处于 maintenance（维护）模式；Readiness 因此必须关闭。
    pub maintenance_active: bool,
    /// 当前部署 operation 的状态，来自调用方的 lifecycle/replay 结果。
    pub operation_state: OperationState,
    /// operation 所属阶段，必须等于由 `operation_state` 映射出的阶段。
    pub operation_phase: OperationPhase,
    /// operation 的截止时间是否已过；过期时总体状态为 Unknown，不等于自动取消 operation。
    pub operation_deadline_expired: bool,
    /// 调用方提供的组件证据摘要；只校验格式并进入 input digest，不解析其引用对象。
    pub component_evidence_digest: String,
    /// 对除自身以外全部输入字段计算的摘要，用于发现字段修改或传输漂移；不是签名。
    pub input_digest: String,
}

impl HealthAggregationInput {
    /// 检查输入合同是否完整、自洽且可被当前版本 reducer 接受。
    ///
    /// 按顺序校验固定 schema/version、文本/游标/时间/epoch/generation 的基本边界、
    /// source event ID 的非空唯一性、两个引用摘要的格式、operation state/phase 映射，
    /// 最后检查 `input_digest`。返回值是稳定错误字符串，Daemon adapter 会把它包装成
    /// `PortError::Failed`。
    ///
    /// ⚠ 校验通过仅表示输入字段之间符合本地合同。Provider/lease/startup 的值仍来自调用方；
    /// 这里不加载被摘要引用的对象，不验证签名、权限、来源或事实新鲜度。
    pub fn validate(&self) -> Result<(), String> {
        // 这些是所有状态分支的共同前置条件。尤其 cursor/epoch/generation 必须非零，
        // 避免把“缺少身份/版本”误当成一个合法的初始状态。
        if self.schema != HEALTH_AGGREGATION_SCHEMA
            || self.version != HEALTH_AGGREGATION_VERSION
            || self.component.trim().is_empty()
            || self.component.len() > MAX_HEALTH_COMPONENT
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || self.observed_at_ms == 0
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.generation == 0
            || self.projection_cursor == 0
            || self.lease_authority_epoch == 0
            || self.lease_data_epoch == 0
        {
            return Err("health_aggregation_input_header_invalid".to_owned());
        }
        // Event ID 用字符串做集合键，因为该领域 ID 类型需要在序列化表示上比较唯一性。
        // 重复 source 引用会让报告看起来绑定了更多事实，但实际仍指向同一事件，因此拒绝。
        let mut ids = std::collections::BTreeSet::new();
        if self
            .source_event_ids
            .iter()
            .any(|event_id| !ids.insert(event_id.to_string()))
        {
            return Err("health_aggregation_source_event_duplicate".to_owned());
        }
        // 这里只确认摘要形状，没有对应 report/component 的参数可供内容比对。
        valid_digest(
            &self.component_evidence_digest,
            "health_aggregation_component_evidence_digest",
        )?;
        valid_digest(
            &self.startup_report_digest,
            "health_aggregation_startup_report_digest",
        )?;
        if self.operation_phase != operation_phase(self.operation_state) {
            return Err("health_aggregation_operation_phase_mismatch".to_owned());
        }
        // input_digest 覆盖全部业务字段但不包含自身；重新计算可以发现字段被改后忘记更新
        // 摘要的情况。它是无密钥摘要，不承担签名或身份认证。
        valid_digest(&self.input_digest, "health_aggregation_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("health_aggregation_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 按固定字段集合计算输入指纹，供 `validate()` 和 Daemon probe adapter 绑定修改后的值。
    ///
    /// `json_digest` 对字段序列化结果计算 SHA-256。该摘要能发现意外漂移或错误拼接，但
    /// 任何能构造输入的人也能重新计算它，因此它不能证明字段值来自可信服务端。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "probe": self.probe,
            "component": self.component,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "observed_at_ms": self.observed_at_ms,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "generation": self.generation,
            "startup_status": self.startup_status,
            "eventlog_ready": self.eventlog_ready,
            "projection_cursor": self.projection_cursor,
            "unknown_count": self.unknown_count,
            "lease_active": self.lease_active,
            "lease_authority_epoch": self.lease_authority_epoch,
            "lease_data_epoch": self.lease_data_epoch,
            "lease_fence_valid": self.lease_fence_valid,
            "provider_healthy": self.provider_healthy,
            "provider_evidence_verified": self.provider_evidence_verified,
            "drain_active": self.drain_active,
            "maintenance_active": self.maintenance_active,
            "operation_state": self.operation_state,
            "operation_phase": self.operation_phase,
            "operation_deadline_expired": self.operation_deadline_expired,
            "startup_report_digest": self.startup_report_digest,
            "component_evidence_digest": self.component_evidence_digest,
        }))
    }
}

/// 对一个 `HealthAggregationInput` 的确定性健康报告。
///
/// 报告同时包含易于消费的状态/组件快照和 `admission_allowed` 派生结果。该布尔值只
/// 表达“若采用本合同，Readiness 是否为 Ok”，不会自动接入 ControlPlane 的授权链或
/// 阻止某个请求。报告是否可信仍取决于调用方输入事实的来源；本类型只能校验报告与输入
/// 是否按同一算法绑定。
///
/// `Serialize` / `Deserialize` 支持跨接口传输，`deny_unknown_fields` 让未识别字段在解码时
/// 直接失败，避免旧版本静默丢弃健康字段。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthAggregationReport {
    /// 报告 schema 固定名称。
    pub schema: String,
    /// 报告 schema 版本。
    pub version: SchemaVersion,
    /// 产生本报告的 `HealthAggregationInput.input_digest`。
    pub input_digest: String,
    /// 这份报告回答的 probe 类型。
    pub probe: HealthProbeKind,
    /// 由输入证据和 probe 规则计算出的总体状态。
    pub status: SignalStatus,
    /// 仅当 probe 是 Readiness 且 status 为 Ok 时为 true；它是结果字段，不执行接纳拦截。
    pub admission_allowed: bool,
    /// 报告对应的源事件游标。
    pub source_cursor: u64,
    /// 输入中的投影游标。
    pub projection_cursor: u64,
    /// `source_cursor.saturating_sub(projection_cursor)`；投影游标超前时单独用状态表示，lag 为零。
    pub projection_lag: u64,
    /// 输入中的未知副作用数量，便于调用者无需再解析 snapshot limitations 即可读到关键门槛。
    pub unknown_count: u64,
    /// 按 lease 活跃、fence 和 authority/data epoch 算出的有效租约冲突标记。
    pub lease_conflict: bool,
    /// 兼容既有健康输出 schema 的组件、能力位、限制和快照摘要。
    pub snapshot: HealthSnapshot,
    /// 对报告其余字段计算的 SHA-256 指纹；不是签名或真实性证明。
    pub report_digest: String,
}

impl HealthAggregationReport {
    /// 校验输入、由输入重建报告，再验证并绑定最终结果。
    ///
    /// 失败可能来自输入字段、输入摘要、组件快照构造/验证或报告绑定；错误以 Domain
    /// 稳定字符串返回。函数只在内存中创建值，不读外部证据、不写 EventStore，也不改变
    /// admission。必须经完整重算而不是仅检查 digest，才能确保所有派生字段符合规则。
    pub fn evaluate(input: &HealthAggregationInput) -> Result<Self, String> {
        input.validate()?;
        let report = build_report(input)?;
        report.validate_against(input)?;
        Ok(report)
    }

    /// 检查报告字段之间的结构关系及其自摘要。
    ///
    /// 例如校验 snapshot 的 probe/status 与报告相等、`admission_allowed` 是否等于
    /// `Readiness && Ok`、lag 是否与两个游标一致。此方法没有输入参数，不能独自证明报告
    /// 来自哪份 typed facts；要检查输入绑定必须调用 `validate_against()`。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HEALTH_AGGREGATION_SCHEMA
            || self.version != HEALTH_AGGREGATION_VERSION
            || self.snapshot.probe != self.probe
            || self.snapshot.status != self.status
            || self.admission_allowed
                != (self.probe == HealthProbeKind::Readiness && self.status == SignalStatus::Ok)
            || self.source_cursor == 0
            || self.projection_cursor == 0
            || self.projection_lag != self.source_cursor.saturating_sub(self.projection_cursor)
        {
            return Err("health_aggregation_report_header_invalid".to_owned());
        }
        valid_digest(&self.input_digest, "health_aggregation_report_input_digest")?;
        self.snapshot.validate()?;
        valid_digest(&self.report_digest, "health_aggregation_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("health_aggregation_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 重新按 `input` 计算期望报告，并要求当前报告与期望结果逐字段相等。
    ///
    /// 这能拒绝“报告自己摘要正确、但把 `input_digest` 换成另一输入摘要”的拼接情况。
    /// 输入的 startup/provider/component digest 仍只是调用方提供的字符串，本方法不会去
    /// 获取摘要指向的原始对象，也不证明外部世界状态。
    pub fn validate_against(&self, input: &HealthAggregationInput) -> Result<(), String> {
        input.validate()?;
        self.validate()?;
        let expected = build_report(input)?;
        if self != &expected {
            return Err("health_aggregation_report_binding_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算报告指纹，绑定 status、派生布尔值、游标、lease 结论和完整 snapshot。
    ///
    /// `report_digest` 本身不进入哈希输入以避免摘要递归；摘要是无密钥的一致性指纹，
    /// 不是签名或对任何输入事实的认证。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "input_digest": self.input_digest,
            "probe": self.probe,
            "status": self.status,
            "admission_allowed": self.admission_allowed,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_lag": self.projection_lag,
            "unknown_count": self.unknown_count,
            "lease_conflict": self.lease_conflict,
            "snapshot": self.snapshot,
        }))
    }
}

/// 把通过格式校验的输入转换为完整报告。
///
/// 仅由 `HealthAggregationReport::evaluate()` 与 `validate_against()` 调用。这里按固定顺序
/// 计算总体状态、snapshot 能力/组件/限制，再计算输入绑定和报告摘要，最后由调用者做
/// 回验。它不查询这些输入所描述的真实 adapter，也不把报告写到持久化存储。
fn build_report(input: &HealthAggregationInput) -> Result<HealthAggregationReport, String> {
    // saturating_sub 避免 projection_cursor > source_cursor 时无符号减法下溢；此时 lag
    // 被表示为 0，但“游标超前”必须通过单独的布尔值传入状态判定，不能误判为追平。
    let projection_lag = input.source_cursor.saturating_sub(input.projection_cursor);
    let projection_ahead = input.projection_cursor > input.source_cursor;
    let status = status_for(input, projection_lag, projection_ahead);
    let mut snapshot = HealthSnapshot::new(
        input.component.clone(),
        status,
        input.source_cursor,
        input.source_event_ids.clone(),
        input.observed_at_ms,
    )?;
    snapshot.probe = input.probe;
    let lease_conflict = effective_lease_conflict(input);
    // capability map 是输入事实的可读投影，便于 API/UI 展示；填入这些布尔值不会真正
    // 检查服务、打开 admission 或对租约执行 CAS。
    snapshot.capabilities = BTreeMap::from([
        (
            "startup_complete".to_owned(),
            input.startup_status == StartupCoordinatorStatus::Ready,
        ),
        ("eventlog_ready".to_owned(), input.eventlog_ready),
        (
            "provider_evidence_verified".to_owned(),
            input.provider_evidence_verified,
        ),
        ("lease_conflict".to_owned(), lease_conflict),
        ("drain_active".to_owned(), input.drain_active),
        ("maintenance_active".to_owned(), input.maintenance_active),
    ]);
    let eventlog_state = if input.eventlog_ready {
        ComponentHealthState::Healthy
    } else {
        ComponentHealthState::Unavailable
    };
    let projector_state = if projection_ahead || projection_lag > 0 {
        ComponentHealthState::Degraded
    } else {
        ComponentHealthState::Healthy
    };
    // provider_healthy 只有在 evidence_verified 为 true 时才有意义。未验证时用 Unknown，
    // 不能把未经核验的“健康”自报提升为 Healthy；Provider 状态只在 Readiness 分支阻断总状态。
    let provider_state = if !input.provider_evidence_verified {
        ComponentHealthState::Unknown
    } else if input.provider_healthy {
        ComponentHealthState::Healthy
    } else {
        ComponentHealthState::Degraded
    };
    let lease_state = if lease_conflict {
        ComponentHealthState::Unknown
    } else {
        ComponentHealthState::Healthy
    };
    snapshot.components = BTreeMap::from([
        (
            "eventlog".to_owned(),
            ComponentHealth::new(
                "eventlog",
                "health.v1",
                eventlog_state,
                Some(input.source_cursor),
                (!input.eventlog_ready).then_some("eventlog_not_ready".to_owned()),
            )?,
        ),
        (
            "projector".to_owned(),
            ComponentHealth::new(
                "projector",
                "health.v1",
                projector_state,
                Some(input.projection_cursor),
                (projection_ahead || projection_lag > 0)
                    .then_some("projection_lag_or_cursor_ahead".to_owned()),
            )?,
        ),
        (
            "provider".to_owned(),
            ComponentHealth::new(
                "provider",
                "health.v1",
                provider_state,
                None,
                if !input.provider_evidence_verified {
                    Some("provider_health_unverified".to_owned())
                } else if !input.provider_healthy {
                    Some("provider_unhealthy".to_owned())
                } else {
                    None
                },
            )?,
        ),
        (
            "lease".to_owned(),
            ComponentHealth::new(
                "lease",
                "health.v1",
                lease_state,
                None,
                lease_conflict.then_some("lease_conflict".to_owned()),
            )?,
        ),
    ]);
    // 限制列表是面向读取者的解释信息，按代码顺序保留前 16 项。总体 status 由 status_for()
    // 独立决定，不能因为列表截断而认为未列出的风险已经消失。
    let mut limitations = Vec::new();
    if input.startup_status != StartupCoordinatorStatus::Ready {
        limitations.push("startup_incomplete".to_owned());
    }
    if !input.eventlog_ready {
        limitations.push("eventlog_not_ready".to_owned());
    }
    if projection_ahead || projection_lag > 0 {
        limitations.push("projection_lag_or_cursor_ahead".to_owned());
    }
    if input.unknown_count > 0 {
        limitations.push("unknown_effect_present".to_owned());
    }
    if lease_conflict {
        limitations.push("lease_conflict".to_owned());
    }
    if input.operation_state == OperationState::Unknown {
        limitations.push("operation_unknown".to_owned());
    }
    if input.operation_deadline_expired {
        limitations.push("operation_deadline_expired".to_owned());
    }
    if !input.provider_evidence_verified {
        limitations.push("provider_health_unverified".to_owned());
    } else if !input.provider_healthy {
        limitations.push("provider_unhealthy".to_owned());
    }
    if input.drain_active {
        limitations.push("draining_no_new_admission".to_owned());
    }
    if input.maintenance_active {
        limitations.push("maintenance_no_new_admission".to_owned());
    }
    limitations.truncate(MAX_HEALTH_LIMITATIONS);
    snapshot.limitations = limitations;
    snapshot.snapshot_digest = snapshot.digest();
    snapshot.validate()?;
    // admission_allowed 只是按 probe/status 推导出的报告字段。真正的 admission 决策仍须
    // 经过部署 gate / ControlPlane authority 链路；本 reducer 没有调用那个执行边界。
    let mut report = HealthAggregationReport {
        schema: HEALTH_AGGREGATION_SCHEMA.to_owned(),
        version: HEALTH_AGGREGATION_VERSION,
        input_digest: input.input_digest.clone(),
        probe: input.probe,
        status,
        admission_allowed: input.probe == HealthProbeKind::Readiness && status == SignalStatus::Ok,
        source_cursor: input.source_cursor,
        projection_cursor: input.projection_cursor,
        projection_lag,
        unknown_count: input.unknown_count,
        lease_conflict,
        snapshot,
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    Ok(report)
}

/// 按优先级计算总体 probe 状态。
///
/// 【判断顺序】
/// 1. `unknown_count`、有效 lease 冲突、operation Unknown 或 deadline 过期先返回 Unknown；
///    即使同时 drain，也不能用一个普通 Degraded 掩盖结果不确定性。
/// 2. Drain/Maintenance 返回 Degraded，表示当前不适合接纳新工作。
/// 3. projection 游标落后/超前或 EventStore 未 ready 返回 Degraded。
/// 4. Startup 要求 StartupCoordinator Ready；Readiness 额外要求已验证且健康的 Provider、
///    无 drain/maintenance；Liveness 在前述共用门槛通过后返回 Ok。
///
/// Provider 未验证会把 Provider 组件置为 Unknown，但不会单独让 Startup/Liveness 的总体
/// status 失败；它仅是 Readiness 的额外条件。这是因为存活探针与“可接收新工作”不是同一
/// 问题。此函数不执行 admission，只返回状态值。
fn status_for(
    input: &HealthAggregationInput,
    projection_lag: u64,
    projection_ahead: bool,
) -> SignalStatus {
    let lease_conflict = effective_lease_conflict(input);
    if input.unknown_count > 0 || lease_conflict {
        return SignalStatus::Unknown;
    }
    if input.operation_state == OperationState::Unknown || input.operation_deadline_expired {
        return SignalStatus::Unknown;
    }
    if matches!(
        input.probe,
        HealthProbeKind::Drain | HealthProbeKind::Maintenance
    ) {
        return SignalStatus::Degraded;
    }
    if projection_ahead || projection_lag > 0 || !input.eventlog_ready {
        return SignalStatus::Degraded;
    }
    match input.probe {
        HealthProbeKind::Startup => {
            if input.startup_status == StartupCoordinatorStatus::Unknown {
                SignalStatus::Unknown
            } else if input.startup_status == StartupCoordinatorStatus::Ready {
                SignalStatus::Ok
            } else {
                SignalStatus::Degraded
            }
        }
        HealthProbeKind::Readiness => {
            if input.startup_status == StartupCoordinatorStatus::Ready
                && input.provider_evidence_verified
                && input.provider_healthy
                && !input.drain_active
                && !input.maintenance_active
                && input.operation_state != OperationState::Unknown
                && !input.operation_deadline_expired
                && !lease_conflict
            {
                SignalStatus::Ok
            } else {
                SignalStatus::Degraded
            }
        }
        HealthProbeKind::Liveness => SignalStatus::Ok,
        HealthProbeKind::Drain | HealthProbeKind::Maintenance => SignalStatus::Degraded,
    }
}

/// 从输入中的 lease 声明推导是否存在有效租约冲突。
///
/// authority/data epoch 不一致、租约不活跃或 fence 无效任一情况都会返回冲突；
/// `status_for()` 将它优先解释为 Unknown，因为当前 holder 是否仍受正确权限和 fencing
/// 保护无法由这些输入证明。该函数只比较输入字段，不访问租约服务，也不检查真实 lease
/// 是否存在、是否已过期或是否曾被另一个进程持有。
fn effective_lease_conflict(input: &HealthAggregationInput) -> bool {
    input.lease_authority_epoch != input.authority_epoch
        || input.lease_data_epoch != input.data_epoch
        || !input.lease_active
        || !input.lease_fence_valid
}

/// 将 operation 状态映射到必须与输入一同携带的 phase。
///
/// Created/Preflight 属于 Preflight，Draining 与 Executing 各自对应同名阶段，Completed、
/// Failed、Cancelled、Unknown 都属于 Terminal。`HealthAggregationInput::validate()` 用
/// 这个映射拒绝 state/phase 不一致的组合，防止下游根据不同字段得出不同生命周期结论。
/// 这里仅验证单个快照，不验证状态转移历史是否合法。
fn operation_phase(state: OperationState) -> OperationPhase {
    match state {
        OperationState::Created | OperationState::Preflight => OperationPhase::Preflight,
        OperationState::Draining => OperationPhase::Draining,
        OperationState::Executing => OperationPhase::Executing,
        OperationState::Completed
        | OperationState::Failed
        | OperationState::Cancelled
        | OperationState::Unknown => OperationPhase::Terminal,
    }
}

/// 验证调用方传入的摘要符合 `sha256:<64 位十六进制>` 的格式。
///
/// 此函数只验证外观：不会重新读取源对象、重算 `startup_report_digest` 或
/// `component_evidence_digest`，也不检查哈希是否由受信任主体签发。前缀要求精确小写
/// `sha256:`，十六进制部分接受大小写字母；不符合时使用调用者提供的 field 名生成稳定
/// 错误码。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
