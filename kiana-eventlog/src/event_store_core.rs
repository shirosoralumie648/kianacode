//! 内存与 JSONL EventStore 共用的事件校验和追加计划 helper。
//!
//! 【本模块在数据流中的位置】
//! ```text
//! MemoryEventLog / JsonlEventLog 收到 append 或事务重放请求
//!       ↓
//! plan_append / plan_idempotent_append 在当前 JournalState 上校验候选 RuntimeEvent
//!       ↓
//! 返回 Append / Replay，或返回 PortError；调用 adapter 决定何时更新内存/文件
//! ```
//!
//! 这里把事件身份、secret-free、audit/governance 契约、聚合流顺序和幂等键规则集中起来，避免
//! 两种存储 adapter 接受不同的事件。所有 `plan_*` 函数只计算结果，不修改事件切片或写文件；
//! 文件同步、内存锁和提交顺序仍由调用它们的 adapter 负责。

use crate::audit_contract;
use crate::governance_contract;
use kiana_domain::RuntimeEvent;
use kiana_ports::PortError;

#[derive(Debug, PartialEq)]
pub(crate) enum AppendPlan {
    /// 候选事件通过检查，需要由 adapter 真正追加后再更新索引。
    Append(RuntimeEvent),
    /// 幂等键命中且语义内容一致，返回此前存储的事件，不再追加。
    Replay(RuntimeEvent),
}

/// 检查一条待持久化事件是否满足共用的领域、安全和审计契约。
///
/// 【调用者】`JournalState::plan_legacy` 以及事务计划路径会调用；通常此时 adapter 已取得
/// 自己的锁，因此此方法虽不写状态，却可能在锁内完成结构校验。
/// 【检查顺序】先检查 aggregate metadata 的完整性；带现代流元数据且载荷为对象时再按 domain
/// registry 校验；随后检查事件身份链接、audit/governance 契约，再检查载荷中是否有禁止保存的
/// secret：`validate_secret_free()` 拒绝任何会被结构化脱敏改写的字段（含 JWT、URL userinfo）；
/// INT-13 的 `scan_secret_value` 再拒绝脱敏不会重写的文本形状（`sk-` token 前缀、
/// `proxy-authorization:` 文本标记、NUL 或超限输入）。
/// 【副作用】现代契约校验失败时会向 stderr 写一条只含 kind、错误码和载荷 key 名称的日志；
/// 随后返回 `PortError::Failed`，不会让计划继续走到 apply/写盘。
/// 【兼容边界】旧事件和非对象 payload 不进入对象 registry 分支，但仍经过其余适用检查；
/// 两道 secret 检查对所有事件都执行，因此兼容路径也不会把未脱敏的凭证形状写入存储。
/// 是否应进一步收紧 registry 分支属于数据格式迁移决策，不能在这里靠注释暗示格式已统一。
pub(crate) fn validate_event_for_storage(event: &RuntimeEvent) -> Result<(), PortError> {
    validate_stream_metadata(event)?;
    // 仅当事件带 aggregate/stream 版本元数据且载荷是对象时，按当前 domain registry 校验。
    // 老格式事件与非对象 payload 保留兼容路径；它们仍会经过后面的身份、审计、治理和 secret 检查，
    // 不能因为跳过 registry 分支就理解为“完全不校验”。
    if (event.aggregate_type.is_some() || event.stream_version.is_some()) && event.data.is_object()
    {
        kiana_domain::validate_runtime_event(event).map_err(|error| {
            eprintln!(
                "event_contract_validation_failed kind={} error={} keys={:?}",
                event.kind,
                error,
                event
                    .data
                    .as_object()
                    .map(|object| object.keys().collect::<Vec<_>>())
            );
            PortError::Failed(format!("event_contract_{error}"))
        })?;
    }
    event
        .validate_identity_links()
        .map_err(|error| PortError::Failed(format!("event_identity_links_invalid:{error}")))?;
    audit_contract::validate_runtime_event(event).map_err(PortError::Failed)?;
    governance_contract::validate_runtime_event(event).map_err(PortError::Failed)?;
    kiana_domain::validate_secret_free(&event.data)
        .map_err(|error| PortError::Failed(format!("eventlog_{error}")))?;
    // `validate_secret_free` 的差异比较覆盖脱敏会改写的形状；这里再跑一次共享哨兵扫描，
    // 补上 `sk-` token 前缀、`proxy-authorization:` 文本标记与 NUL/超限输入，
    // finding 不含原始文本。
    kiana_domain::scan_secret_value_with_limit(
        kiana_domain::SecretScanChannel::Event,
        &event.data,
        kiana_domain::MAX_JOURNAL_EVENT_BYTES,
    )
    .map_err(|finding| PortError::Failed(format!("eventlog_{finding}")))?;
    Ok(())
}

/// 验证聚合流的类型、ID 和版本元数据必须同时提供或同时缺省。
///
/// 老事件三个字段都缺省时，后续按 `request_id` 兼容排序；只提供其中一部分时拒绝，因为缺失的
/// 聚合身份会让 `same_stream` 无法准确判断事件属于哪条流。流版本必须大于 0。失败返回
/// `event_stream_metadata_invalid`，调用方应停止追加。
fn validate_stream_metadata(event: &RuntimeEvent) -> Result<(), PortError> {
    if event.aggregate_type.is_none()
        && event.aggregate_id.is_none()
        && event.stream_version.is_none()
    {
        return Ok(());
    }

    let valid = matches!(
        (
            event.aggregate_type.as_deref(),
            event.aggregate_id.as_deref(),
            event.stream_version,
        ),
        (Some(aggregate_type), Some(aggregate_id), Some(stream_version))
            if !aggregate_type.trim().is_empty()
                && !aggregate_id.trim().is_empty()
                && stream_version > 0
    );
    if !valid {
        return Err(PortError::Failed(
            "event_stream_metadata_invalid".to_owned(),
        ));
    }
    Ok(())
}

/// 校验一次不使用幂等键重放的单事件追加计划。
///
/// 【输入】现有事件切片、候选事件，以及可选的目标流 expected version。
/// 【流程】先执行共同事件校验，再比对预期流版本，最后检查 event ID 唯一及 stream version
/// 严格递增。
/// 【输出】成功返回原候选事件供 adapter 追加；任何检查失败均返回 PortError，且本函数不改变
/// 输入切片或存储状态。
pub(crate) fn plan_append(
    events: &[RuntimeEvent],
    event: RuntimeEvent,
    expected_version: Option<u64>,
) -> Result<RuntimeEvent, PortError> {
    validate_event_for_storage(&event)?;
    reject_expected_version(events, &event, expected_version)?;
    reject_conflicts(events, &event)?;
    Ok(event)
}

pub(crate) fn plan_idempotent_append(
    events: &[RuntimeEvent],
    event: RuntimeEvent,
    expected_version: Option<u64>,
) -> Result<AppendPlan, PortError> {
    validate_event_for_storage(&event)?;
    // 先处理已提交的幂等重试，再核对 expected version：原请求提交后流版本通常已经递增，
    // 若先做 CAS，合法重试会被误报为冲突，而不是返回原事件。
    let key = validate_idempotency_key(&event)?;
    if let Some(existing) = events
        .iter()
        .find(|existing| existing.idempotency_key.as_deref() == Some(key))
    {
        ensure_idempotent_match(existing, &event)?;
        return Ok(AppendPlan::Replay(existing.clone()));
    }

    plan_append(events, event, expected_version).map(AppendPlan::Append)
}

/// 读取并检查 RuntimeEvent 上的幂等键，供 adapter 在进入写锁前快速拒绝缺失值。
///
/// 只拒绝缺失或全空白的 key，不会 trim 或改写合法 key；因此调用方重试时必须传入完全相同的
/// 键文本。返回借用的原字符串，避免产生第二份可能不一致的 key。
pub(crate) fn validate_idempotency_key(event: &RuntimeEvent) -> Result<&str, PortError> {
    event
        .idempotency_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| PortError::Failed("event_idempotency_key_required".to_owned()))
}

/// 拒绝重复 EventId 或目标流中不递增的事件版本。
///
/// ID 重复表示两条事实占用了同一事件身份；版本小于或等于已有最大版本表示流顺序会倒退或
/// 重复。两者都返回 `PortError::Conflict`，调用方应重新读取当前状态再决定是否重新构造事件。
pub(crate) fn reject_conflicts(
    events: &[RuntimeEvent],
    event: &RuntimeEvent,
) -> Result<(), PortError> {
    if events
        .iter()
        .any(|existing| existing.event_id == event.event_id)
    {
        return Err(PortError::Conflict("event_id_duplicate".to_owned()));
    }
    let version = stream_version(event);
    if events
        .iter()
        .any(|existing| same_stream(existing, event) && stream_version(existing) >= version)
    {
        return Err(PortError::Conflict(
            "event_sequence_not_monotonic".to_owned(),
        ));
    }
    Ok(())
}

fn reject_expected_version(
    events: &[RuntimeEvent],
    event: &RuntimeEvent,
    expected_version: Option<u64>,
) -> Result<(), PortError> {
    // expected_version 是候选事件所属流的当前版本，不是 EventStore 全局游标。新流尚无事件时
    // 版本按 0 处理；只要不匹配就先返回 Conflict，不会继续到唯一性检查或追加。
    let Some(expected_version) = expected_version else {
        return Ok(());
    };
    let current_version = events
        .iter()
        .filter(|existing| same_stream(existing, event))
        .map(stream_version)
        .max()
        .unwrap_or(0);
    if current_version != expected_version {
        return Err(PortError::Conflict(
            "event_stream_version_mismatch".to_owned(),
        ));
    }
    Ok(())
}

/// 判断两条事件是否属于同一顺序流。
///
/// 现代事件以 `(aggregate_type, aggregate_id)` 标识流；没有聚合元数据的兼容旧事件则以
/// `request_id` 临时分组。完整性前置检查会拒绝只带部分聚合元数据的事件，避免新旧规则混用。
fn same_stream(left: &RuntimeEvent, right: &RuntimeEvent) -> bool {
    let left_has_metadata = left.aggregate_type.is_some() || left.aggregate_id.is_some();
    let right_has_metadata = right.aggregate_type.is_some() || right.aggregate_id.is_some();
    if left_has_metadata || right_has_metadata {
        return matches!(
            (
                left.aggregate_type.as_deref(),
                left.aggregate_id.as_deref(),
                right.aggregate_type.as_deref(),
                right.aggregate_id.as_deref(),
            ),
            (Some(left_type), Some(left_id), Some(right_type), Some(right_id))
                if left_type == right_type && left_id == right_id
        );
    }
    left.request_id == right.request_id
}

/// 取事件的聚合流版本；旧事件未提供时退回请求内 sequence 作为兼容顺序。
fn stream_version(event: &RuntimeEvent) -> u64 {
    event.stream_version.unwrap_or(event.sequence)
}

fn ensure_idempotent_match(
    existing: &RuntimeEvent,
    candidate: &RuntimeEvent,
) -> Result<(), PortError> {
    // 幂等键相同还不足以证明是重放：逐个比较会影响事件事实含义的字段，防止调用方复用一个
    // key 提交不同 payload/身份链接。event_id 本身不参与比较，因为重试可重新生成候选 ID；
    // 命中时 adapter 返回 existing，保留最初实际存储的那一个 EventId。
    let same_payload = existing.request_id == candidate.request_id
        && existing.sequence == candidate.sequence
        && existing.kind == candidate.kind
        && existing.data == candidate.data
        && existing.redaction_profile == candidate.redaction_profile
        && existing.payload_recoverable == candidate.payload_recoverable
        && existing.data_epoch == candidate.data_epoch
        && existing.artifact_refs == candidate.artifact_refs
        && existing.command_id == candidate.command_id
        && existing.correlation_id == candidate.correlation_id
        && existing.causation_event_id == candidate.causation_event_id
        && existing.parent_event_id == candidate.parent_event_id
        && existing.aggregate_type == candidate.aggregate_type
        && existing.aggregate_id == candidate.aggregate_id
        && existing.stream_version == candidate.stream_version;
    if !same_payload {
        return Err(PortError::Conflict(
            "event_idempotency_key_payload_mismatch".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::RequestId;
    use serde_json::Value;

    fn event(request_id: RequestId, sequence: u64, kind: &str) -> RuntimeEvent {
        RuntimeEvent::new(request_id, sequence, kind, Value::Null).unwrap()
    }

    #[test]
    fn plans_append_and_replay_without_mutating_storage() {
        let request_id = RequestId::new();
        let candidate = event(request_id, 1, "accepted").with_idempotency_key("request:1");
        let plan = plan_idempotent_append(&[], candidate.clone(), Some(0)).unwrap();
        assert_eq!(plan, AppendPlan::Append(candidate.clone()));

        let replay = plan_idempotent_append(
            std::slice::from_ref(&candidate),
            candidate.clone(),
            Some(99),
        )
        .unwrap();
        assert_eq!(replay, AppendPlan::Replay(candidate));
    }

    #[test]
    fn preserves_cas_and_payload_conflicts() {
        let request_id = RequestId::new();
        let first = event(request_id, 1, "accepted").with_idempotency_key("request:1");
        let stale = event(request_id, 2, "completed").with_idempotency_key("request:2");
        assert_eq!(
            plan_idempotent_append(std::slice::from_ref(&first), stale, Some(0)).unwrap_err(),
            PortError::Conflict("event_stream_version_mismatch".to_owned())
        );

        let changed = event(request_id, 1, "different").with_idempotency_key("request:1");
        assert_eq!(
            plan_idempotent_append(&[first], changed, None).unwrap_err(),
            PortError::Conflict("event_idempotency_key_payload_mismatch".to_owned())
        );
    }

    #[test]
    fn requires_keys_only_for_idempotent_appends() {
        let request_id = RequestId::new();
        let candidate = event(request_id, 1, "accepted");
        assert!(plan_append(&[], candidate.clone(), None).is_ok());
        assert_eq!(
            plan_idempotent_append(&[], candidate, None).unwrap_err(),
            PortError::Failed("event_idempotency_key_required".to_owned())
        );
    }
}
