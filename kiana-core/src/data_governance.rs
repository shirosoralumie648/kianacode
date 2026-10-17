//! 数据治理：把「一条执行收据（receipt）里的数据从哪来、能不能被复制、什么时候必须消失」落成事实。
//!
//! # 解决的是一个具体的问题
//!
//! receipt 里会出现数据载荷（payload）。载荷一旦进了收据，就可能被复制、导出、缓存、重放。
//! 这个文件负责让「哪些数据可以出现在 receipt 里、引用了哪些源事件、
//! 封存之后还能不能读」都变成**可核对的事实**，而不是约定。
//!
//! # 它操作的是「引用」，不是「内容」
//!
//! 这个文件里出现的基本都是 `*Ref`、`*Digest`、`*Plan`——对象引用与摘要，
//! 而不是实际字节。这与 `artifacts.rs` 里那条原则一致：
//! **事件与收据里放摘要，字节留在它该在的地方。**
//!
//! # 在系统中的位置
//!
//! 本文件属于 `kiana-core` 的控制平面（`ControlPlane`）业务层。它从事件存储接口
//!（`EventStorePort`）读取已提交的事件事实，调用 `kiana-domain` 中的治理值对象生成
//! 投影（projection，即从事实重建的查询视图）；涉及撤销时，还会让
//! `ApprovalPort` 失效并通知 Runner（规范执行循环）停止关联运行。它不负责解析 HTTP/CLI 请求，
//! 也不直接删除源事件或数据文件。
//!
//! ```text
//! 已通过 ControlPlane 授权的 data.governance 请求
//!   -> dispatch.rs 记录 execution.prepared
//!   -> 本文件 prepare_data_revocation()
//!      -> EventStore：提交撤销事实和 run.cancelling
//!      -> ApprovalPort：使项目旧审批失效
//!      -> Runner + capability broker（能力代理器）：取消项目运行并核对停止结果
//!
//! run 事件 -> receipts.rs -> receipt_data_binding_from_events() -> receipt 中的引用/摘要
//! 上层调用 ControlPlane::data_governance_snapshot()（当前 workspace 尚无生产调用点）
//!   -> EventStore 事件 + DataPolicy -> project_data_governance_snapshot() -> 治理状态快照
//! ```
//!
//! 最相关的配套文件是 `receipts.rs`（收据生成与读取）、`dispatch.rs`（能力请求进入
//! ControlPlane 的执行阶段）、`recovery.rs`（用治理标记阻止旧运行恢复）、
//! `workspace_checkpoints.rs`（恢复工作区时复用项目失效和停止流程），以及
//! `kiana-domain/src/governance.rs`（快照、绑定、传播计划等数据结构及其校验规则）。
//! 治理输入主要来自已提交事件和上层提供的 `DataPolicy`；输出是供收据、恢复和后续
//! 传播流程消费的摘要、引用、快照或错误。
use super::*;
use kiana_domain::{
    CapabilityErrorCode, DataGovernanceSnapshot, DataPayloadState, DataPolicy, DataPropagationPlan,
    ImmutableEventSeal, ReceiptAuditMetadata, ReceiptDataBinding, ReceiptPayloadRef, RuntimeEvent,
    MAX_SOURCE_EVENT_IDS,
};
use serde_json::Value;
use std::collections::HashSet;

// `run_data_revoked_via_cursor()` 是兼容只支持分页读取的 EventStore（事件存储）的后备路径。
// 全量读取会一次把整本事件账本放进内存；游标读取则分批检查，但也必须有总上限，
// 否则一个异常巨大或无限增长的账本会让这次只读查询一直占用资源。
//
// 每页请求 64 条，最多读取 256 页，所以正常情况下最多检查 16,384 条事件。
// 这不是“超过上限就当作没有撤销”：到达上限时会返回错误，调用方必须将结果视为
// 无法确认，而不能据此继续展示或使用可能已撤销的数据。
//
// EventStore 的 `read_from()` 契约要求不拆分已提交的事务帧。因此，如果一帧本身
// 大于 64 条，单页可以多于请求值；但仍不能超过领域层允许的单帧最大事件数。
const REVOCATION_CURSOR_PAGE_EVENTS: usize = 64;
const MAX_REVOCATION_CURSOR_PAGES: usize = 256;
const MAX_REVOCATION_CURSOR_EVENTS: usize =
    REVOCATION_CURSOR_PAGE_EVENTS * MAX_REVOCATION_CURSOR_PAGES;

/// A request-local fold over facts in EventStore commit order. Both read capabilities must
/// validate the same project/run binding before claiming that complete history has no revocation.
/// This state is a projection only; it grants no execution authority and changes no event fact.
struct RunRevocationScan {
    run_id: String,
    project_root: Option<std::path::PathBuf>,
    revoked: bool,
}

impl RunRevocationScan {
    fn new(run_id: RunId) -> Self {
        Self {
            run_id: run_id.to_string(),
            project_root: None,
            revoked: false,
        }
    }

    fn observe(&mut self, event: &RuntimeEvent) -> Result<(), PortError> {
        if event.kind == "data.revocation_requested" {
            // An unknown target cannot prove that a revocation belongs to another project.
            // Validate even facts preceding authorization; only valid, later matching facts
            // change visibility under the existing chronological governance contract.
            let root = event.data["project_root"]
                .as_str()
                .filter(|root| !root.trim().is_empty())
                .ok_or_else(|| PortError::Failed("data_revocation_event_invalid".to_owned()))?;
            if let Some(authorized_root) = &self.project_root {
                self.revoked |= authorized_root == &ControlPlane::canonical_project_root(root);
            }
            return Ok(());
        }
        if event.kind != "run.authorized" {
            return Ok(());
        }
        let payload_matches = event.data["run_id"].as_str() == Some(self.run_id.as_str());
        let stream_matches = event.aggregate_type.as_deref() == Some("run")
            && event.aggregate_id.as_deref() == Some(self.run_id.as_str());
        if !payload_matches && !stream_matches {
            return Ok(());
        }
        // All-absent metadata is the existing legacy envelope. Any modern binding must be
        // complete and identify this exact run, including the payload, on every authorization.
        let metadata = (
            event.aggregate_type.as_deref(),
            event.aggregate_id.as_deref(),
            event.stream_version,
        );
        let valid_binding = metadata == (None, None, None)
            || matches!(metadata, (Some("run"), Some(id), Some(version))
                if id == self.run_id.as_str() && version > 0);
        if !payload_matches || !valid_binding {
            return Err(PortError::Failed(
                "data_revocation_run_binding_conflict".to_owned(),
            ));
        }
        let root = event.data["project_root"]
            .as_str()
            .filter(|root| !root.trim().is_empty())
            .ok_or_else(|| PortError::Failed("data_revocation_authorization_missing".to_owned()))?;
        let root = ControlPlane::canonical_project_root(root);
        match &self.project_root {
            Some(previous) if previous != &root => Err(PortError::Failed(
                "data_revocation_run_binding_conflict".to_owned(),
            )),
            Some(_) => Ok(()),
            None => {
                self.project_root = Some(root);
                Ok(())
            }
        }
    }

    fn finish(self) -> Result<bool, PortError> {
        if self.project_root.is_none() {
            return Err(PortError::Failed(
                "data_revocation_authorization_missing".to_owned(),
            ));
        }
        Ok(self.revoked)
    }
}

/// 根据生成收据所用的运行事件，生成收据的数据来源绑定。
///
/// 【作用】
/// 将收据摘要、项目标识、数据策略和事件来源整理成 `ReceiptDataBinding`，供收据说明
/// “来源于哪些事件、引用了哪些对象”。该绑定只包含 ID、对象引用和摘要，不复制事件
/// 或制品（artifact）的原始字节，也不会授予读取载荷（payload）的权限。
///
/// 【调用者与数据流】
/// `receipts.rs::receipt_from_events()` 在构造运行收据时调用本函数。输入的 `events`
/// 是同一运行的已提交事件；成功结果会序列化进 receipt。若这里返回错误，当前调用者
/// 会将绑定替换成 `state: unknown` 的占位信息，而不是丢弃整张收据。
///
/// 【输入与输出】
/// - `receipt_digest`：已经生成的收据摘要，用于把绑定固定到这张收据。
/// - `project_ref`：收据所属项目的引用；调用方传入规范化后的项目路径。
/// - `policy`：数据治理策略（`DataPolicy`），必须先通过领域层校验。
/// - `events`：收据对应的源事件；不能为空。
/// - 成功时返回摘要绑定，初始载荷状态固定为 `Unknown`，等待新鲜的治理快照判断
///   是否允许读取内容。
///
/// 【失败情况与副作用】
/// 空事件、无效策略、引用数量超限或领域对象校验失败时返回字符串错误；本函数没有
/// I/O、日志或持久化副作用，也不会重试、禁用账号或改变数据状态。
///
/// 【容易误解的字段】
/// `source_digest` 和 `payload_digest` 当前都由事件的 `data` 计算；本函数没有读取
/// `object_ref` 背后的 artifact 字节，所以不能把这个摘要解释成对实际文件内容的重新
/// 校验。`seen` 只用于去重 `source_event_ids`，不负责去重 `payload_refs`。
///
/// 【两个防护，各挡一类】
/// - `events.is_empty()` → `receipt_data_source_empty`：
///   一份**指不出来源**的 receipt 无法被复核。没有源事件的 receipt
///   和凭空写下的 receipt 在证据能力上是一样的。
/// - `payload_refs.len() >= MAX_RECEIPT_PAYLOAD_REFS` → `receipt_payload_ref_limit`：
///   引用数量也必须有上限，否则「一份收据」可以变成整个存储的索引。
///
/// 【为什么源事件 ID 要去重】
/// 同一个事件 ID 可能因重复投递出现在输入列表多次。源 ID 列表表达的是“哪些不同
/// 事件为收据提供了来源”，所以重复 ID 不应重复占位；但对象引用列表仍逐条按照事件
/// 中的 `artifact_refs` 生成，两者的去重规则并不相同。
pub fn receipt_data_binding_from_events(
    receipt_digest: &str,
    project_ref: &str,
    policy: &DataPolicy,
    events: &[RuntimeEvent],
) -> Result<ReceiptDataBinding, String> {
    policy.validate()?;
    if events.is_empty() {
        return Err("receipt_data_source_empty".to_owned());
    }
    let mut source_event_ids = Vec::with_capacity(events.len().min(MAX_SOURCE_EVENT_IDS));
    let mut seen = HashSet::new();
    let mut payload_refs = Vec::new();
    // 源事件 ID 数量有上限，避免一张 receipt 成为不受控的全局索引；同一批事件中的
    // artifact 引用仍逐项检查，并由下面独立的 payload-ref 上限保护。
    for event in events.iter().take(MAX_SOURCE_EVENT_IDS) {
        if seen.insert(event.event_id) {
            source_event_ids.push(event.event_id);
        }
        for reference in &event.artifact_refs {
            // 这里限制的是收据要携带的对象引用数，不是源事件数；命中上限就报错，
            // 不静默截断，以免收据看起来完整却漏掉部分来源对象。
            if payload_refs.len() >= kiana_domain::MAX_RECEIPT_PAYLOAD_REFS {
                return Err("receipt_payload_ref_limit".to_owned());
            }
            payload_refs.push(ReceiptPayloadRef {
                object_ref: reference.clone(),
                source_digest: kiana_domain::json_digest(&event.data),
                // 事件可显式声明自己所属的数据 epoch；旧事件没有该字段时，使用本次
                // 传入策略的 epoch。该值是版本边界，不是时间戳或 grant 数量。
                data_epoch: event.data_epoch.unwrap_or(policy.data_epoch),
                payload_digest: kiana_domain::json_digest(&event.data),
            });
        }
    }
    // 这里记录输入事件的逻辑位置数；转换前先限制到 u64 可表示范围，避免 usize 到
    // u64 的窄平台转换溢出。它不是 `source_event_ids` 去重后的数量。
    let source_cursor = events.len().min(u64::MAX as usize) as u64;
    let redaction_profile = events
        .iter()
        .rev()
        .find_map(|event| event.redaction_profile.clone())
        // 历史事件都没有声明脱敏配置时保留明确的“未声明/未脱敏”标记，不能伪装成
        // 某个已知脱敏 profile。
        .unwrap_or_else(|| "unredacted-source-profile".to_owned());
    let metadata = ReceiptAuditMetadata {
        project_ref: project_ref.to_owned(),
        // 领域层要求 revision 为正数；旧策略仍为 0 时，收据边界使用最小有效值 1。
        policy_revision: policy.revision.max(1),
        data_epoch: policy.data_epoch,
        source_cursor,
        source_event_ids,
        redaction_profile,
        retained: true,
    };
    // 收据里有对象引用，不等于调用者因此获得读取权限。即使源事件已成功提交，
    // 没有新鲜治理快照时，载荷状态也必须保持 Unknown。
    ReceiptDataBinding::new(
        receipt_digest,
        metadata,
        payload_refs,
        DataPayloadState::Unknown,
    )
}

/// 根据治理快照和新的删除标记（tombstone）生成有界的数据传播计划。
///
/// 【作用与调用关系】
/// 这是领域层 `DataPropagationPlan::from_snapshot()` 的核心层入口。它把“哪个项目、
/// 从哪个数据版本代次（epoch）迁移到哪个版本、由哪条删除标记（tombstone）触发、当前事件游标到哪里”
/// 组合成一份带摘要的计划，供后续 adapter 逐个处理 receipt、memory、index 等派生
/// 存储。生成计划不等于已经完成删除或传播；每个目标的实际处理仍需由后续流程提供
/// 回执。当前 workspace 中没有生产模块直接调用本函数，它是公开导出的纯计算能力，
/// 不能据此推断派生存储传播已经接入。
///
/// 【输入与输出】
/// `snapshot` 是服务端策略和事件事实投射出的治理快照；`previous_epoch` 是更新前的
/// 版本；`kind` 说明失效原因；`tombstone_digest` 固定本次失效事实；`observed_at_ms`
/// 记录观察时间。成功时返回带目标清单和计划摘要的值对象，领域校验失败时返回错误。
/// 本函数纯计算，不读取存储、不执行删除，也不产生重试或日志副作用。
pub fn plan_data_propagation(
    snapshot: &DataGovernanceSnapshot,
    previous_epoch: u64,
    kind: &str,
    tombstone_digest: &str,
    observed_at_ms: u64,
) -> Result<DataPropagationPlan, String> {
    DataPropagationPlan::from_snapshot(
        snapshot,
        previous_epoch,
        kind,
        tombstone_digest,
        observed_at_ms,
    )
}

/// 为治理投影记录不可变源事件的封存边界。
///
/// 【作用】
/// 将输入事件的 ID、事件游标、数据版本代次（epoch）和加密配置名称交给
/// `ImmutableEventSeal::new()`，由领域层生成并验证一份 seal。撤销、删除和过期可以让
/// 派生视图或 payload 不再可读，但这里的 seal 只记录源事件边界，不改写或删除已提交
/// 的 EventLog 事实。当前 workspace 中没有生产模块直接调用本函数；生成 seal 也不代表
/// 已经实施了加密或完成了物理删除。
///
/// 【输入、输出与失败】
/// 输入事件不能为空；源事件 ID 最多收集 `MAX_SOURCE_EVENT_IDS` 个，游标记录输入事件
/// 数量。`data_epoch` 和 `encryption_profile` 是封存元数据，不代表本函数真的加密数据。
/// 领域对象构造或校验失败时返回错误；本函数没有文件、网络或存储副作用。
pub fn seal_governance_events(
    events: &[RuntimeEvent],
    data_epoch: u64,
    encryption_profile: &str,
) -> Result<ImmutableEventSeal, String> {
    if events.is_empty() {
        return Err("immutable_event_seal_source_empty".to_owned());
    }
    // seal 保存有界数量的来源 ID，避免大型输入生成无限膨胀的 seal；source_cursor 仍记
    // 录完整输入长度，因此 seal 同时说明“实际记录了多少来源 ID”和“扫描到了哪里”。
    let source_event_ids = events
        .iter()
        .map(|event| event.event_id)
        .take(MAX_SOURCE_EVENT_IDS)
        .collect::<Vec<_>>();
    ImmutableEventSeal::new(
        events.len().min(u64::MAX as usize) as u64,
        data_epoch,
        source_event_ids,
        encryption_profile,
    )
}

/// 将数据策略与已提交的失效事件投射成当前治理状态快照。
///
/// 【作用与位置】
/// 这是本文件的纯投影入口：先由 `kiana-domain` 根据 `DataPolicy` 计算各数据来源的
/// 初始状态，再用事件账本中的撤销/恢复事实覆盖尚不确定或已撤销的数据状态。它不
/// 删除、改写 EventLog，也不直接触碰 receipt、memory 或索引中的字节。
///
/// 【调用者与数据流】
/// `ControlPlane::data_governance_snapshot()` 会把从 EventStore 读到的事件传入此函数；
/// 收据构造器则通过 `receipt_data_binding_from_events()` 建立独立的来源绑定。输入事件
/// 应是**当前项目范围内**、按连续游标顺序排列的事件；`project_ref` 和服务端策略由
/// 上层调用者提供，`now_ms` 用于领域策略判断保留期限是否已过。
///
/// ⚠ 注意：本函数会读取输入序列中的所有撤销/恢复事实，但不会根据 `project_ref` 检查
/// 每条事件的 `project_root`。因此，传入前必须由调用方完成项目范围过滤；全局事件流
/// 中另一个项目的失效事实可能影响当前快照。当前 `ControlPlane` 包装方法会直接读取
/// 整个 EventStore，因此使用该包装方法时尤其需要注意这个作用域边界。
///
/// 【核心流程】
/// 1. 拒绝空历史、零起点、过多事件、重复事件 ID 和游标跳号；
/// 2. 按事件顺序识别待完成的撤销/恢复，并收集治理结果中已撤销的来源路径；
/// 3. 调用领域层生成策略快照，再验证策略 epoch 没有被投影过程意外改变；
/// 4. 若失效操作尚无完成结果，将相关 payload 状态降为 `Unknown`；若结果明确列出已
///    撤销来源，则标记这些来源为 `Revoked`，并相应标记派生存储状态；
/// 5. 最后重算快照摘要并运行完整结构校验。
///
/// 【错误如何传播】
/// 任一输入边界、cursor 连续性、epoch 或快照校验失败都会返回字符串错误。调用此
/// 函数的 ControlPlane 包装方法会把它转换为 `PortError::Failed`，再由上层请求处理流程
/// 负责返回错误；它不会把损坏或不完整的历史伪装成“数据可用”。
pub fn project_data_governance_snapshot(
    policy: &DataPolicy,
    project_ref: &str,
    events: &[RuntimeEvent],
    now_ms: u64,
) -> Result<DataGovernanceSnapshot, String> {
    if events.is_empty() {
        return Err("data_governance_source_empty".to_owned());
    }
    // 事件读取适配器可能显式携带源游标；没有携带时，本投影按从 1 开始的连续事件
    // 列表解释。后续逐条核对游标，是为了防止缺页或拼接了不同区间的历史被当成完整源。
    // EventCursor 从 1 计数，0 表示日志开头；缺少字段的输入按第一个逻辑事件位于 1 处理。
    let first_cursor = events
        .first()
        .and_then(|event| event.data.get("source_cursor"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if first_cursor == 0 || events.len() > MAX_SOURCE_EVENT_IDS {
        return Err("data_governance_source_cursor_invalid".to_owned());
    }
    let mut ids = Vec::with_capacity(events.len());
    let mut seen = HashSet::new();
    let mut revoked_sources = HashSet::new();
    let mut pending_invalidation = false;
    let mut last_cursor = first_cursor;
    for (index, event) in events.iter().enumerate() {
        // 同一事件 ID 再次出现说明来源序列有重复，不能把它当成两个独立事实静默投影。
        if !seen.insert(event.event_id.to_string()) {
            return Err("data_governance_source_event_duplicate".to_owned());
        }
        ids.push(event.event_id);
        // 用“起始游标 + 切片内偏移”重建每个事件应处于的位置，后续如有显式游标就逐一
        // 比较；checked_add 防止超长历史令游标溢出后绕回较小数字。
        let expected = first_cursor
            .checked_add(index as u64)
            .ok_or_else(|| "data_governance_source_cursor_overflow".to_owned())?;
        last_cursor = expected;
        if let Some(cursor) = event.data.get("source_cursor").and_then(Value::as_u64) {
            if cursor != expected {
                return Err("data_governance_source_cursor_gap".to_owned());
            }
        }
        // 撤销或恢复请求意味着相关数据可能已经失效，但实际治理结果还未确认。
        // 在看到明确的治理结果事件前，下面会把 payload 标为 Unknown，而不是假定成功
        // 删除，也不是继续宣称它可用。
        if event.kind == "data.revocation_requested" || event.kind == "workspace.restore_requested"
        {
            pending_invalidation = true;
        }
        // 治理结果通常包在执行结果的 output 里；其他事件则直接检查自身 data。
        // 两种位置都支持，避免把同一种 schema 因事件包装层级不同而漏掉。
        let output = if event.kind == "execution.result_committed" {
            event
                .data
                .get("result")
                .and_then(|result| result.get("output"))
        } else {
            Some(&event.data)
        };
        if let Some(output) = output {
            if output.get("schema").and_then(Value::as_str)
                == Some("kiana.data-governance-result.v1")
            {
                // 只有读到带预期 schema 的结果才清除 pending 标记；同时仅从结果的
                // policy.revoked_sources 收集被明确确认撤销的来源。
                pending_invalidation = false;
                if let Some(paths) = output
                    .pointer("/policy/revoked_sources")
                    .and_then(Value::as_array)
                {
                    revoked_sources
                        .extend(paths.iter().filter_map(Value::as_str).map(str::to_owned));
                }
            }
        }
    }
    // 领域层按策略本身计算 Available / Expired / Revoked 等初始状态；本文件随后把
    // EventLog 中更近的失效事实叠加到快照上。epoch fence 确保投影没有拿旧策略生成新视图。
    let mut snapshot =
        kiana_domain::project_data_governance(policy, project_ref, last_cursor, ids, now_ms)?;
    kiana_domain::MemoryProjectionFence::validate_epoch(policy.data_epoch, snapshot.data_epoch)?;
    // PD-18：同一份历史必须能重建内存读取面的删除索引，并且只能绑定到当前策略 epoch。
    // 重建失败意味着读取面无法被证明已与治理事实对齐，投影必须失败，而不是当作“没有删除”。
    let deletion_index = project_memory_deletion_index(policy, project_ref, events, now_ms)?;
    if pending_invalidation {
        // 请求已经提交、完成结果尚未出现时，无法证明派生数据仍安全可用，因此统一降为
        // Unknown。消费者应拒绝据此读取 payload，直到有新的治理事实完成确认。
        for observation in &mut snapshot.observations {
            observation.payload = DataPayloadState::Unknown;
        }
        for state in snapshot.derived_store_states.values_mut() {
            *state = DataPayloadState::Unknown;
        }
    } else if !revoked_sources.is_empty() {
        // 只有结果明确列出路径时才精确标记对应来源；若至少有一个来源已撤销，依赖它们
        // 的派生存储也不能继续被整体报告为可用。
        for observation in &mut snapshot.observations {
            if revoked_sources.contains(&observation.source_ref) {
                observation.payload = DataPayloadState::Revoked;
            }
        }
        if snapshot
            .observations
            .iter()
            .any(|observation| observation.payload == DataPayloadState::Revoked)
        {
            for state in snapshot.derived_store_states.values_mut() {
                *state = DataPayloadState::Revoked;
            }
        }
    }
    // 删除索引里还没有回执的检索面（lexical/vector/cache/receipt）不能继续被报告为可用：
    // 未证明的传播按 Unknown 上报，读取面据此拒绝任何复活副本。
    apply_memory_deletion_surface_states(&mut snapshot, &deletion_index);
    snapshot.snapshot_digest = snapshot.digest();
    snapshot.validate()?;
    Ok(snapshot)
}

/// PD-18：从同一份已提交历史重建内存删除索引，并绑定到当前策略 epoch。
///
/// 【作用与调用者】
/// `project_data_governance_snapshot()` 在领域投影之后立即调用它，作为
/// `MemoryProjectionFence::validate_epoch` 之外的第二道一致性检查：内存读取面使用的删除
/// 索引必须能由这份历史重建，且其 epoch/policy 绑定必须与当前策略一致。失败时返回字符串
/// 错误，调用方（ControlPlane）会把它转成 `PortError::Failed`，不会把损坏的历史伪装成
/// “没有删除”。
///
/// 【它不做什么】
/// 不删除、不回填、不重建任何派生存储，也不改写历史事实；只产出一个可核对的值对象。
pub fn project_memory_deletion_index(
    policy: &DataPolicy,
    project_ref: &str,
    events: &[RuntimeEvent],
    now_ms: u64,
) -> Result<kiana_domain::MemoryDeletionIndex, String> {
    policy.validate()?;
    if project_ref.trim().is_empty() {
        return Err("data_governance_project_required".to_owned());
    }
    let mut entries = Vec::new();
    for event in events {
        if event.kind != kiana_domain::MEMORY_FACT_EVENT_KIND {
            continue;
        }
        let fact: kiana_domain::MemoryJournalFact = serde_json::from_value(event.data.clone())
            .map_err(|_| "data_governance_memory_fact_invalid".to_owned())?;
        fact.validate()?;
        if fact.operation != "delete"
            && !matches!(
                fact.record.visibility(),
                kiana_domain::MemoryVisibility::Denied
            )
        {
            continue;
        }
        let deleted_at_ms = fact
            .record
            .reviewed_at_ms
            .filter(|stamp| *stamp > 0)
            .unwrap_or(fact.record.created_at_ms)
            .max(1);
        entries.push(kiana_domain::MemoryDeletionEntry::new(
            fact.record.id.clone(),
            fact.record.collection.clone(),
            fact.record.revision,
            kiana_domain::json_digest(&event.data),
            deleted_at_ms,
        )?);
    }
    let index = kiana_domain::MemoryDeletionIndex::new(
        project_ref,
        policy.revision,
        policy.data_epoch,
        now_ms.max(1),
        kiana_domain::merge_memory_deletion_entries(&[], &entries),
    )?;
    index.bind_policy(policy)?;
    Ok(index)
}

/// 把删除索引中未完成的检索面映射到快照的派生存储状态上。
///
/// lexical/vector/cache/receipt 四个面分别对应 bm25-index / dense-index / cache / receipt；
/// 只有确实存在该键时才会降级，`Available` 绝不会被写成“已传播”。
fn apply_memory_deletion_surface_states(
    snapshot: &mut DataGovernanceSnapshot,
    deletion_index: &kiana_domain::MemoryDeletionIndex,
) {
    for surface in deletion_index.pending_surfaces() {
        let store = match surface {
            "lexical" => "bm25-index",
            "vector" => "dense-index",
            "cache" => "cache",
            "receipt" => "receipt",
            _ => continue,
        };
        if let Some(state) = snapshot.derived_store_states.get_mut(store) {
            *state = DataPayloadState::Unknown;
        }
    }
}

/// 使用较短的公开名称，执行完整的数据治理快照投影。
///
/// 【作用与调用者】
/// 这是 `project_data_governance_snapshot()` 的兼容/便捷入口，没有额外转换或副作用。
/// 当前仓库中的验证代码会直接调用它；需要显式强调 snapshot 的调用者可调用较长名称。
///
/// 【输入、输出与失败】
/// 四个参数会原样转交给完整实现；返回的快照、校验错误和该实现完全一致，不进行 I/O。
pub fn project_data_governance(
    policy: &DataPolicy,
    project_ref: &str,
    events: &[RuntimeEvent],
    now_ms: u64,
) -> Result<DataGovernanceSnapshot, String> {
    // 保留一个较短的公开名称，统一委托给完整实现；参数和错误语义完全相同。
    project_data_governance_snapshot(policy, project_ref, events, now_ms)
}
impl ControlPlane {
    /// 原子记录一次项目失效事实，并给该项目仍在运行的任务发出取消状态。
    ///
    /// 【调用者】
    /// 数据撤销由 `prepare_data_revocation()` 调用；工作区恢复由
    /// `workspace_checkpoints.rs` 调用。权限或其他控制面变化也会复用停止流程，但不会
    /// 必然调用本方法。
    ///
    /// 【输入与输出】
    /// - `root`：要失效的项目目录；通过 `canonical_project_root()` 与授权事件中的路径
    ///   比较，避免路径书写形式不同造成漏匹配。
    /// - `kind` 与 `data`：本次要追加的事件类型和事实载荷，例如撤销或恢复请求。
    /// - 成功时返回 `Ok(())`，表示事件批次已由 EventStore 确认提交；存储、读取或版本
    ///   冲突错误会向调用者传播，后续审批失效和运行取消不会在本方法中执行。
    ///
    /// 【核心流程与设计原因】
    /// 先从授权事件找出属于该项目的运行，再检查每个运行当前 turn 是否已经终结。对
    /// 未终结的运行构造 `run.cancelling`，并与项目失效事实放进同一个 `TransitionBatch`。
    /// 批次携带 request/run 聚合的期望版本，由 `commit_confirmed()` 通过 EventStore 原子
    /// 比较并提交；这样并发写入发生时不会只留下撤销事实却漏掉对应的取消状态。
    /// 终态运行不会被重复改写，已提交的源事件也不会被删除。
    pub(crate) async fn begin_project_invalidation(
        &self,
        root: &str,
        kind: &str,
        data: Value,
    ) -> Result<(), CoreError> {
        // 这次操作的 run 集合来自历史授权事实，而不是仅依赖可能已过期的内存 session 表。
        let all = self.events.read_all().await?;
        let runs = all
            .iter()
            .filter(|event| {
                event.kind == "run.authorized"
                    && event.data["project_root"].as_str().is_some_and(|value| {
                        Self::canonical_project_root(value) == Self::canonical_project_root(root)
                    })
            })
            .filter_map(|event| event.data["run_id"].as_str().and_then(RunId::parse_str))
            .collect::<std::collections::HashSet<_>>();
        let command_id = RequestId::new();
        // 一个独立 command_id 贯穿整个原子批次。request 聚合从版本 0 开始，后面为每个
        // 活跃 run 加入它当前的流版本，作为并发修改时的 compare-and-swap 前置条件。
        let mut versions = vec![kiana_domain::AggregateVersion::new(
            "request",
            command_id.to_string(),
            0,
        )];
        let mut events = vec![RuntimeEvent::new(command_id, 1, kind, data.clone())?
            .with_stream_metadata("request", command_id.to_string(), 1)];
        for run_id in runs {
            let run = self.events.read_stream("run", &run_id.to_string()).await?;
            // 只检查当前 prompt 开始后的事件，避免旧 turn 的终态事件误把当前运行判成终止。
            let turn = run
                .iter()
                .rposition(|event| event.kind == "run.prompt")
                .unwrap_or(0);
            if run[turn..].iter().any(|event| {
                matches!(
                    event.kind.as_str(),
                    "run.completed" | "run.failed" | "run.cancelled" | "run.result_unknown"
                )
            }) {
                // 该 turn 已经有终态事实，无需再追加取消事件。
                continue;
            }
            let version = run
                .iter()
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0);
            versions.push(kiana_domain::AggregateVersion::new(
                "run",
                run_id.to_string(),
                version,
            ));
            events.push(
                RuntimeEvent::new(
                    command_id,
                    events.len() as u64 + 1,
                    "run.cancelling",
                    json!({"run_id":run_id,"reason":kind,"project_root":root}),
                )?
                .with_stream_metadata("run", run_id.to_string(), version + 1),
            );
        }
        super::dispatch::commit_confirmed(
            self.events.as_ref(),
            kiana_domain::TransitionBatch {
                command_id,
                command_digest: kiana_domain::json_digest(&json!({"kind":kind,"data":data})),
                expected_versions: versions,
                events,
            },
        )
        .await?;
        // 只有上面的批次确认提交后才会返回成功；调用者此后才可以撤销审批或停止运行。
        Ok(())
    }

    /// 读取项目最近一次撤销/恢复边界的事件 ID，作为运行恢复时的数据版本标记。
    ///
    /// 【调用者】
    /// `recovery.rs` 在创建运行快照和继续运行之前调用它，并把返回值与快照中的旧标记
    /// 比较。注意这里的 `data_epoch` 是不透明的字符串令牌（最近相关事件 ID），不是
    /// `DataPolicy::data_epoch` 中的数字计数器。
    ///
    /// 【输入、输出与失败】
    /// 输入为项目路径。找到最近的 `data.revocation_requested`、`workspace.restore_requested`
    /// 或 `workspace.restored` 事件时返回其 event ID 字符串；从未发生这些事件时返回
    /// `None`。读取事件账本失败会返回 `CoreError`，不会假装没有发生过失效。
    /// 本方法只读账本，不产生副作用。
    pub(crate) async fn data_epoch(&self, project_root: &str) -> Result<Option<String>, CoreError> {
        let events = self.events.read_all().await?;
        Ok(events
            .iter()
            .rev()
            .find(|event| {
                matches!(
                    event.kind.as_str(),
                    "data.revocation_requested"
                        | "workspace.restore_requested"
                        | "workspace.restored"
                ) && event.data["project_root"].as_str().is_some_and(|root| {
                    Self::canonical_project_root(root) == Self::canonical_project_root(project_root)
                })
            })
            .map(|event| event.event_id.to_string()))
    }
    /// 查询某次运行所属项目是否在该运行获准之后发生过数据撤销。
    ///
    /// 【作用】
    /// 收据、记忆提案、版本追踪和执行结果投影在使用运行数据前，调用此方法确认其来源
    /// 项目没有被撤销。这里检查的是事件账本中的治理事实，不会自行删除事件或载荷。
    ///
    /// 【调用者】
    /// 当前调用点包括 `receipts.rs`、`memory_proposals.rs`、`versioning.rs` 和
    /// `parity.rs`。这些调用方分别决定是否阻止展示收据、创建记忆提案、追踪版本，
    /// 或把执行结果降级为 `ResultUnknown`。
    ///
    /// 【输入与输出】
    /// - `run_id`：`run.authorized` 事件记录的运行 ID，用来找到这次运行对应的项目。
    /// - `Ok(true)`：该运行获准后，账本中出现了同一规范化项目路径的撤销事件。
    /// - `Ok(false)`：走全量读取时没有找到该运行，或没有找到其获准之后的撤销事件；
    ///   走游标回退时，只有找到授权事件且没有后续撤销事件才会返回 `false`。
    /// - `Err(...)`：账本读取失败或无法确认完整历史；调用方收到错误，不能把它当作
    ///   “未撤销”。本方法不做重试、不禁用账号，也不直接生成用户界面消息。
    ///
    /// 【核心流程】
    /// 先尝试适配器提供的全量读取，再找该运行的授权事件，并只检查该事件之后发生的
    /// 撤销事实。仅当适配器明确返回 `event_store_read_all_unsupported` 时，才改用有界
    /// 游标扫描；其他读取错误会原样转换为核心层错误并向调用链上传播。
    ///
    /// 【为什么区分“不支持”和“读取失败”？】
    /// 如果把任何错误都当成需要回退，真实存储故障可能被另一条读取路径掩盖；如果把
    /// 不支持全量读取误当成空账本，又会把“无法检查”错误地解释成“没有撤销”。
    pub(crate) async fn run_data_revoked(&self, run_id: RunId) -> Result<bool, CoreError> {
        let events = match self.events.read_all().await {
            Ok(events) => events,
            Err(PortError::Failed(reason)) if reason == "event_store_read_all_unsupported" => {
                return self.run_data_revoked_via_cursor(run_id).await;
            }
            Err(error) => return Err(error.into()),
        };
        let mut scan = RunRevocationScan::new(run_id);
        for event in &events {
            scan.observe(event)?;
        }
        Ok(scan.finish()?)
    }

    /// 在 EventStore 不支持全量读取时，分页扫描账本以回答撤销查询。
    ///
    /// 【作用与调用者】
    /// 这是 `run_data_revoked()` 的内部兼容路径，只能在 `read_all()` 明确返回“不支持”
    /// 时进入。对于能找到授权事件的历史，它按照相同的事件顺序和项目路径规则判断
    /// 撤销。但当前实现对“完整账本中没有该运行的授权事件”处理不同：全量读取路径
    /// 返回 `Ok(false)`，游标路径返回 `data_revocation_authorization_missing` 错误。
    /// 这项差异可能影响只支持游标读取的适配器，具体待确认；本任务不改变该行为。
    ///
    /// 【输入与输出】
    /// - `run_id`：要检查的运行身份；授权事件中的项目路径是撤销匹配的依据。
    /// - `Ok(true)`：完整扫描到末尾，且找到匹配项目的后续撤销事件。
    /// - `Ok(false)`：完整扫描到末尾，找到运行授权，但没有匹配的后续撤销事件。
    /// - `Err(...)`：存储不支持游标读取、读取失败、页或事件身份不可信、找不到授权事实，
    ///   或扫描预算耗尽。错误会传回 `run_data_revoked()` 的调用者，避免把不完整历史
    ///   当成“没有撤销”。
    ///
    /// 【核心流程】
    /// 1. 检查 EventStore 是否声明支持游标读取；
    /// 2. 从事件游标 0 开始，每次请求一页；
    /// 3. 检查页大小、游标连续性、事件 ID 唯一性和事件自身的身份/结构约束；
    /// 4. 遇到目标运行的 `run.authorized` 时记录其规范化项目路径；
    /// 5. 只把之后相同项目路径上的撤销事件计入结果；
    /// 6. 只有确认抵达完整历史末尾时才返回布尔结果。
    ///
    /// 【为什么必须验证每一页？】
    /// 游标读取是多个独立存储调用组成的扫描。重复页、跳号页或损坏事件都可能使扫描
    /// 漏掉撤销事实；因此任何无法证明连续、有效的结果都会失败关闭，而不会猜测答案。
    async fn run_data_revoked_via_cursor(&self, run_id: RunId) -> Result<bool, CoreError> {
        if !self.events.capabilities().cursor_reads {
            return Err(PortError::Unavailable(
                "data_revocation_cursor_reads_unsupported".to_owned(),
            )
            .into());
        }
        // EventCursor 的 0 代表账本起点；read_from(cursor, limit) 返回 cursor 之后的一页，
        // 因此从 0 开始才能检查包含最早授权事件在内的全部历史。
        let mut cursor: kiana_domain::EventCursor = 0;
        let mut seen = HashSet::new();
        let mut scan = RunRevocationScan::new(run_id);
        // 每页最多请求 64 条，最多读 256 页；上面的常量注释说明这个总预算如何得出。
        // 有界扫描保证兼容路径不会为了一个查询无上限地遍历增长中的全局事件账本。
        for _ in 0..MAX_REVOCATION_CURSOR_PAGES {
            let page = self
                .events
                .read_from(cursor, REVOCATION_CURSOR_PAGE_EVENTS)
                .await?;
            // 64 是本查询主动请求的页大小；但 EventStore 会保留完整事务帧，不能为了
            // 凑足页大小而把一笔原子提交拆开。因此单页可以更大，最大值由领域层的
            // `MAX_TRANSITION_EVENTS`（单次事务允许携带的事件数）约束。
            if page.events.len() > kiana_domain::MAX_TRANSITION_EVENTS {
                return Err(PortError::Failed(
                    "data_revocation_cursor_page_size_invalid".to_owned(),
                )
                .into());
            }
            // 页必须确实推进游标，且 `page.cursor` 必须等于本页覆盖到的逻辑事件总数。
            // 空页或跳号/倒退都不能证明账本已经完整读完；若把它们当作末尾，可能漏掉
            // 后续撤销事实。`read_from()` 不拆事务帧，所以页长度不必恰好等于请求值。
            if page.events.is_empty()
                || cursor.checked_add(page.events.len() as u64) != Some(page.cursor)
            {
                return Err(PortError::Failed("data_revocation_cursor_invalid".to_owned()).into());
            }
            // `seen` 记录整次扫描中已经出现过的 EventId。这里限制的是累计扫描量，
            // 而不是当前页大小；饱和加法也避免极端长度值在加法溢出后绕过预算检查。
            if seen.len().saturating_add(page.events.len()) > MAX_REVOCATION_CURSOR_EVENTS {
                return Err(
                    PortError::Failed("data_revocation_cursor_event_limit".to_owned()).into(),
                );
            }
            for event in &page.events {
                // Event ID（事件 ID）是单条事件的身份。分页结果若重复包含同一身份，说明读到的
                // 历史不能被当作一次连续、无重复的账本扫描来使用。
                if !seen.insert(event.event_id) {
                    return Err(PortError::Failed(
                        "data_revocation_cursor_duplicate_event".to_owned(),
                    )
                    .into());
                }
                // 有值不等于可信：nil UUID、事件/请求身份链接不一致或 runtime schema 无效，
                // 都会导致运行与撤销事实无法可靠关联，所以一律当作损坏历史处理。
                if event.event_id.as_uuid().is_nil()
                    || event.request_id.as_uuid().is_nil()
                    || event.validate_identity_links().is_err()
                    || kiana_domain::validate_runtime_event(event).is_err()
                {
                    return Err(PortError::Failed(
                        "data_revocation_cursor_event_invalid".to_owned(),
                    )
                    .into());
                }
                // 授权事件为运行提供项目归属；从它之后才开始比较撤销事件，和全量读取
                // 路径保持一致。canonical_project_root 会尽量把同一目录的不同写法归一，
                // 避免仅因路径表示不同而漏掉撤销。
                scan.observe(event)?;
            }
            // `cursor` 表示已经检查过的逻辑事件位置。下一页必须从这里继续，不能重复
            // 扫描上一页，也不能跳过中间一段历史。
            cursor = page.cursor;
            if !page.has_more {
                // 只有适配器明确表示没有更多事件，且目标授权事实存在并且绑定一致，
                // 才能返回 true/false。计数和唯一 ID 不能证明跨 stream 的全序；这里
                // 依赖 EventStorePort / PD-31 已有的已提交事实顺序契约。
                return Ok(scan.finish()?);
            }
        }
        Err(PortError::Failed("data_revocation_cursor_page_limit".to_owned()).into())
    }

    /// 在 Broker 真正执行数据治理能力前，提交撤销边界并停止受影响的旧工作。
    ///
    /// 【调用者与调用时机】
    /// `dispatch.rs` 在能力请求已通过 ControlPlane 授权、且 `execution.prepared` 已提交后，
    /// 在向 capability broker 派发前调用本方法。它只处理 operation 为
    /// `data.governance` 且 action 为 `delete`、`revoke` 或 `expire` 的请求；其他请求
    /// 返回 `Ok(())`，让正常派发流程继续。
    ///
    /// 【输入与副作用】
    /// `request` 中的 `project_root` 和 `actor_id` 是撤销事实的归属信息；`request_id` 和
    /// `grant_id` 被写入事件载荷。处理撤销时依次：提交项目失效/取消事件、使项目审批
    /// 失效、停止同项目运行。只有三步都成功才返回 `Ok(())`，随后 Broker 才有机会执行
    /// 具体治理 handler；任一步失败都会转成 dispatch 错误并阻止本次派发继续。
    ///
    /// 【为什么先做准备步骤】
    /// 在实际删除或修改派生存储前，先建立可核对的撤销事实并停止仍可能读取旧数据的
    /// 任务，避免并发运行继续使用已经进入撤销流程的数据。
    pub(crate) async fn prepare_data_revocation(
        &self,
        request: &CapabilityRequest,
    ) -> Result<(), CoreError> {
        if request.operation != "data.governance"
            || !matches!(
                request.arguments["action"].as_str(),
                Some("delete" | "revoke" | "expire")
            )
        {
            return Ok(());
        }
        let root = request.arguments["project_root"]
            .as_str()
            .ok_or_else(|| PortError::Failed("governance_project_required".to_owned()))?;
        let actor = request.arguments["actor_id"]
            .as_str()
            .ok_or_else(|| PortError::Failed("governance_actor_required".to_owned()))?;
        self.begin_project_invalidation(root,"data.revocation_requested",json!({
            "project_root":root,"actor_id":actor,"request_id":request.request_id,"grant_id":request.arguments["grant_id"],
        })).await?;
        self.approvals
            .invalidate_project(root, "data_revoked")
            .await?;
        self.stop_project_runs(root, "data_revoked").await
    }

    /// 停止指定项目仍在运行的任务，并为取消结果留下事件记录。
    ///
    /// 【调用者与用途】
    /// 数据撤销、工作区恢复和权限变化等控制面流程会调用此方法；本文件中的
    /// `prepare_data_revocation()` 是其中一个入口。它只处理当前进程中登记的 session
    /// binding，再通过持久事件状态和 Runner/capability supervisor 确认每次取消结果。
    ///
    /// 【输入与输出】
    /// `root` 是要停止任务所属的项目目录，`reason` 会进入取消命令和结果事件。所有任务
    /// 都已停止或已经处于可接受的终态时返回 `Ok(())`；读取存储、记录事件、归位 Cell
    /// 或等待取消确认失败时返回错误。若不能确认外部 capability 已停止，返回包含
    /// `result_unknown` 的错误，并将非终态运行记录为 `ResultUnknown`，不能声称副作用肯定
    /// 没有发生。
    ///
    /// 【核心流程】
    /// 1. 复制项目匹配的 session/binding，避免持有同步锁跨越 `.await`；
    /// 2. 查明 run 是否已终结，并读取等待中的审批调用和最近 checkpoint；
    /// 3. 为待审批调用记录未执行结果，选出可恢复运行上下文；
    /// 4. 发出本地取消信号和 Runner 取消命令；
    /// 5. 收集 Runner 的工具取消结果，并独立等待 capability supervisor 停止确认；
    /// 6. 清理内存中的待处理调用、写入终态事件、结算 Cell 并解绑 session。
    ///
    /// 【为什么不能只调用 Runner::Cancel？】
    /// Runner 负责 Agent loop，但工具 handler 可能正由独立的 capability supervisor 执行。
    /// 收到 Runner 取消事件不等于外部进程/handler 已停止，所以必须同时等待另一侧的
    /// stop 确认；缺少任何一侧证据时，结果都只能是未知。
    pub(crate) async fn stop_project_runs(
        &self,
        root: &str,
        reason: &str,
    ) -> Result<(), CoreError> {
        // 先在短暂持锁期间复制匹配项目的 session/binding；后面包含多个 .await，不能把
        // 同步 Mutex 锁跨越异步等待持有，否则会阻塞其他控制面线程并可能造成死锁。
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|(_, binding)| {
                Self::canonical_project_root(&binding.project_root)
                    == Self::canonical_project_root(root)
            })
            .map(|(session, binding)| (session.clone(), binding.clone()))
            .collect::<Vec<_>>();
        for (session, binding) in sessions {
            let request_id = RequestId::new();
            let mut sequence = 1;
            let terminal = self.run_state(binding.run_id).await?.outcome;
            let pending = self
                .pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .values()
                .find(|pending| pending.run_id == binding.run_id)
                .cloned();
            // pending invocation 表示已发起但仍等待审批/结果的能力调用。保存其身份，
            // 以便在停止 run 时给对应工具调用留下“未执行”的结果事实。
            let snapshot = self
                .events
                .read_all()
                .await?
                .into_iter()
                .rev()
                .filter(|event| {
                    event.kind == "run.snapshot" && event.data["run_id"] == json!(binding.run_id)
                })
                .find_map(|event| {
                    serde_json::from_value::<kiana_domain::RunSnapshot>(
                        event.data["snapshot"].clone(),
                    )
                    .ok()
                });
            if let Some(pending) = &pending {
                self.record_event(request_id,&mut sequence,"run.tool_result",json!({"run_id":binding.run_id,
                    "capability_request_id":pending.request_id,"call_id":pending.request.arguments["call_id"],
                    "result":{"error":format!("cancelled:{reason}"),"not_executed":true,"replay_safe":true},"not_executed":true})).await?;
            }
            // 取消可能发生在等待审批、已持久化 checkpoint 或普通活动 session 等不同阶段：
            // 优先沿用 pending invocation 上下文，其次使用 checkpoint；都不存在时再从
            // session binding 重建最小本地上下文，并采用只读 sandbox。
            let (mut context, sandbox) = if let Some(pending) = pending {
                (pending.context, pending.sandbox)
            } else if let Some(snapshot) = snapshot {
                (snapshot.context, snapshot.sandbox)
            } else {
                let mut context =
                    RequestContext::local(session.as_str(), binding.project_root.clone());
                context.actor_id = binding.actor_id.clone();
                context.role_id = binding.role_id.clone();
                context.department_id = binding.department_id.clone();
                context.cell_id = self.cell_registry.cell_for_run(binding.run_id).await?;
                (context, "read-only".to_owned())
            };
            context.request_id = request_id;
            // 两路取消分别通知本地 capability supervisor 和 Runner。Runner 返回取消事件
            // 不足以证明外部 handler 已停止，因此下面还会等待 capability stop 确认。
            self.signal_cancel(binding.run_id);
            let cancelled = self
                .runner
                .send(RunnerCommand::Cancel {
                    run_id: binding.run_id,
                    reason: reason.to_owned(),
                })
                .await;
            // Runner 只有在成功返回一组都属于目标 run 的事件，且没有非取消类失败时才算
            // 确认。`run_not_found` 仅在原本已有终态的情况下可接受；其他失败不能证明
            // 正在执行的 Agent loop 已停下。
            let runner_confirmed = cancelled.as_ref().is_ok_and(|events| {
                events.iter().all(|event| {
                    event.run_id() == binding.run_id
                        && !matches!(event,RunnerEvent::Failed {error,..}
                    if CapabilityErrorCode::from_reason(error) != CapabilityErrorCode::Cancelled
                        && !(error=="run_not_found"&&terminal.is_some()))
                })
            });
            // `await_capability_stop` 检查能力执行侧；必须同时有它和 Runner 的确认，整个
            // run 才算停止。两者负责不同执行层，不能互相代替。
            let confirmed = self.await_capability_stop(binding.run_id).await && runner_confirmed;
            if let Ok(events) = cancelled {
                for event in events {
                    if let RunnerEvent::ToolCancelled {
                        run_id,
                        request_id: capability_id,
                        call_id,
                        result,
                    } = event
                    {
                        self.record_event(request_id,&mut sequence,"run.tool_result",json!({"run_id":run_id,
                            "capability_request_id":capability_id,"call_id":call_id,"result":result,"not_executed":true})).await?;
                    }
                }
            }
            self.pending_invocations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|_, pending| pending.run_id != binding.run_id);
            if terminal.is_none() {
                // 无法确认停止时必须记录 ResultUnknown，而不能写成 Cancelled；否则后续恢复
                // 会误以为副作用肯定没有继续执行。Cell 状态也随结果同步收敛。
                self.record_terminal_event(request_id,&mut sequence,binding.run_id,
                    if confirmed {"run.cancelled"} else {"run.result_unknown"},
                    json!({"run_id":binding.run_id,"error":format!("{}:{reason}",if confirmed {"cancelled"}else{"result_unknown:stop_unconfirmed"})})).await?;
                self.settle_resumed_cell(
                    &context,
                    binding.run_id,
                    &sandbox,
                    if confirmed {
                        ExecutionStatus::Cancelled
                    } else {
                        ExecutionStatus::ResultUnknown
                    },
                    &mut sequence,
                )
                .await?;
            }
            self.forget_session(&session, binding.run_id);
            if !confirmed || terminal == Some(RunOutcome::ResultUnknown) {
                // 已经发出停止请求但没有完整证据时，向上游返回错误，要求调用方把撤销视作
                // 未完成；不能因为内存 session 已清理就声称外部执行一定停止。
                return Err(
                    PortError::Failed(format!("result_unknown:{reason}_stop_unconfirmed")).into(),
                );
            }
        }
        Ok(())
    }

    /// 使用调用方提供的服务端策略和事件事实，生成一个项目范围的治理快照。
    ///
    /// 【作用与调用者】
    /// 这是供上层 adapter 获取 `DataGovernanceSnapshot` 的公开 `ControlPlane` 方法。本
    /// workspace 当前没有其他生产模块直接调用点；上层必须显式传入已选定的策略和项目，
    /// 本方法不会自行从任意路径加载项目配置。
    ///
    /// 【数据流】
    /// 先通过 `read_all_events()` 读取完整事件日志（EventLog），再交给纯函数
    /// `project_data_governance_snapshot()` 校验游标并叠加撤销事实，最终返回带摘要的
    /// 治理快照。策略由调用者提供，不能通过快照接口修改。
    ///
    /// ⚠ 当前实现把完整事件日志直接传入快照投影，没有按 `project_ref` 过滤事件；而
    /// 投影函数会应用传入序列中的所有项目失效事实。若全局事件中不同项目使用相同的
    /// 相对来源路径，或任一项目存在尚未完成的失效请求，可能串入其他项目的状态。
    /// 这是待审查的实现问题，本注释任务不改变其行为。
    ///
    /// 【失败情况与副作用】
    /// EventStore 不支持全量读取时返回 `data_governance_read_all_unsupported`；存储错误、
    /// 事件不连续或快照校验失败也都会作为 `CoreError` 返回。此方法只读，不会清理文件、
    /// 改变策略、重试或取消运行。
    pub async fn data_governance_snapshot(
        &self,
        policy: &DataPolicy,
        project_ref: &str,
        now_ms: u64,
    ) -> Result<DataGovernanceSnapshot, CoreError> {
        let events = self.read_all_events().await?.ok_or_else(|| {
            PortError::Unavailable("data_governance_read_all_unsupported".to_owned())
        })?;
        project_data_governance_snapshot(policy, project_ref, &events, now_ms)
            .map_err(|error| PortError::Failed(error).into())
    }
}
