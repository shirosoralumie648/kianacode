//! ControlPlane action facade for UI command CAS and durable reconciliation.
//!
//! Surface adapters may validate a cursor before making a request, but this module is the only
//! place that persists the action admission.  Each transition goes through the EventStore atomic
//! journal.  An uncertain commit is never converted to Applied: callers must query the original
//! command ID/idempotency key and use the stored record.

use super::*;
use kiana_domain::{
    derived_request_id, json_digest, AggregateVersion, CommitOutcome, RuntimeEvent,
    UiActionAuthoritySnapshot, UiActionCommand, UiActionJournal, UiActionRecord, UiActionState,
    UI_ACTION_AGGREGATE_TYPE,
};
use serde_json::json;

const UI_ACTION_ACCEPTED_KIND: &str = "ui.action.accepted";
const UI_ACTION_APPLIED_KIND: &str = "ui.action.applied";
const UI_ACTION_REJECTED_KIND: &str = "ui.action.rejected";
const UI_ACTION_UNKNOWN_KIND: &str = "ui.action.unknown";

fn ui_action_error(reason: impl Into<String>) -> CoreError {
    CoreError::Port(PortError::Failed(reason.into()))
}

fn journal_digest(value: &str) -> String {
    value.strip_prefix("sha256:").unwrap_or(value).to_owned()
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn record_from_event(event: &RuntimeEvent) -> Result<UiActionRecord, CoreError> {
    let value = event
        .data
        .get("record")
        .cloned()
        .ok_or_else(|| ui_action_error("ui_action_record_missing"))?;
    let record: UiActionRecord = serde_json::from_value(value)
        .map_err(|_| ui_action_error("ui_action_record_decode_failed"))?;
    record
        .validate_for_query()
        .map_err(|error| ui_action_error(format!("ui_action_record_invalid:{error}")))?;
    Ok(record)
}

async fn read_ui_action_stream(
    events: &dyn EventStorePort,
    idempotency_key: &str,
) -> Result<Vec<RuntimeEvent>, CoreError> {
    events
        .read_stream(UI_ACTION_AGGREGATE_TYPE, idempotency_key)
        .await
        .map_err(CoreError::Port)
}

fn latest_record(events: &[RuntimeEvent]) -> Result<Option<UiActionRecord>, CoreError> {
    events
        .iter()
        .filter(|event| event.kind.starts_with("ui.action."))
        .map(record_from_event)
        .last()
        .transpose()
}

fn action_event(
    action: &UiActionCommand,
    record: &UiActionRecord,
    kind: &str,
    stream_version: u64,
    command_id: RequestId,
    idempotency_suffix: &str,
) -> Result<RuntimeEvent, CoreError> {
    let event = RuntimeEvent::new(
        command_id,
        stream_version,
        kind,
        json!({
            "schema": "kiana.ui-action-event.v1",
            "action": action,
            "record": record,
            "state": record.state,
            "effect_count": record.effect_count,
        }),
    )
    .map_err(|error| ui_action_error(format!("ui_action_event_invalid:{error}")))?
    .with_stream_metadata(
        UI_ACTION_AGGREGATE_TYPE,
        action.idempotency_key.clone(),
        stream_version,
    )
    .with_idempotency_key(format!(
        "ui-action:{}:{idempotency_suffix}",
        action.idempotency_key
    ));
    Ok(event)
}

async fn commit_ui_action_event(
    events: &dyn EventStorePort,
    action: &UiActionCommand,
    record: &UiActionRecord,
    kind: &str,
    stream_version: u64,
    command_id: RequestId,
    idempotency_suffix: &str,
) -> Result<CommitOutcome, CoreError> {
    if !events.supports_atomic_transitions() {
        return Err(ui_action_error("control_journal_required"));
    }
    let event = action_event(
        action,
        record,
        kind,
        stream_version,
        command_id,
        idempotency_suffix,
    )?;
    let batch = kiana_domain::TransitionBatch {
        command_id,
        command_digest: journal_digest(&json_digest(&json!({
            "action": action,
            "record": record,
            "kind": kind,
        }))),
        expected_versions: vec![AggregateVersion::new(
            UI_ACTION_AGGREGATE_TYPE,
            action.idempotency_key.clone(),
            stream_version.saturating_sub(1),
        )],
        events: vec![event],
    };
    events
        .commit_transition(batch)
        .await
        .map_err(CoreError::Port)
}

impl ControlPlane {
    /// Refuse a command whose command ID was already committed under a different idempotency key.
    ///
    /// 【调用者】`admit_ui_action()` 在读取原 action stream 之前。
    /// 【输入/输出】command 的 command ID；若该 ID 已有 command receipt，且其中记录的 ui_action
    /// 聚合不是本命令的幂等键，返回 `ui_action_command_id_reused`，否则返回 Ok。
    /// 【失败】存储不支持 command receipt 读取或读取失败时向上返回 Port 错误；不把读不到解释成可用。
    /// 【为什么需要】按 idempotency key 的 CAS 只能发现同一个键上的重放/冲突；同一个客户端换了幂等
    /// 键却复用旧 command ID 时，两个键各自看起来都是第一条命令。这里用已提交 receipt 的聚合绑定
    /// 把这种身份复用变成明确拒绝，而不是让一次旧命令在新键下第二次准入。
    /// 【边界】不比较 receipt 的完整内容，也不检查 receipt 中的 digest；它只回答"这个 command ID
    /// 是否已经被绑定到另一个幂等键"。
    async fn refuse_reused_ui_action_command(
        &self,
        action: &UiActionCommand,
    ) -> Result<(), CoreError> {
        let receipt = self
            .events
            .read_command(&action.command_id)
            .await
            .map_err(CoreError::Port)?;
        let Some(receipt) = receipt else {
            return Ok(());
        };
        let reused = receipt.versions.iter().any(|version| {
            version.aggregate_type == UI_ACTION_AGGREGATE_TYPE
                && version.aggregate_id != action.idempotency_key
        });
        if reused {
            return Err(ui_action_error("ui_action_command_id_reused"));
        }
        Ok(())
    }

    /// Persist one UI action admission with CAS and idempotency semantics.
    pub async fn admit_ui_action(
        &self,
        action: &UiActionCommand,
        authority: &UiActionAuthoritySnapshot,
        now_unix_ms: u64,
    ) -> Result<UiActionRecord, CoreError> {
        action
            .validate()
            .map_err(|error| ui_action_error(format!("ui_action_invalid:{error}")))?;
        authority
            .validate()
            .map_err(|error| ui_action_error(format!("ui_action_authority_invalid:{error}")))?;
        self.refuse_reused_ui_action_command(action).await?;
        let events = read_ui_action_stream(self.events.as_ref(), &action.idempotency_key).await?;
        if let Some(original) = latest_record(&events)? {
            if original.command_digest != action.command_digest {
                return Err(ui_action_error("ui_action_idempotency_digest_mismatch"));
            }
            if original.owner_id != action.owner_id || original.owner_id != authority.owner_id {
                return Err(ui_action_error("ui_action_owner_mismatch"));
            }
            if original.scope_digest != action.scope_digest
                || original.scope_digest != authority.scope_digest
            {
                return Err(ui_action_error("ui_action_scope_mismatch"));
            }
            // A replay returns the original durable result.  It never dispatches a second effect.
            return Ok(original);
        }
        let mut journal = UiActionJournal::new();
        let (record, replayed) = journal
            .accept(action, authority, now_unix_ms)
            .map_err(|error| ui_action_error(format!("ui_action_rejected:{error}")))?;
        debug_assert!(!replayed);
        let outcome = commit_ui_action_event(
            self.events.as_ref(),
            action,
            &record,
            UI_ACTION_ACCEPTED_KIND,
            1,
            action.command_id,
            "accepted",
        )
        .await?;
        match outcome {
            CommitOutcome::Committed { .. } => Ok(record),
            CommitOutcome::Replayed { .. } => self
                .read_original_record(&action.idempotency_key)
                .await
                .map(|value| value.ok_or_else(|| ui_action_error("ui_action_record_missing")))?,
            CommitOutcome::Conflict { .. } => Err(ui_action_error("ui_action_cas_conflict")),
            CommitOutcome::Unknown { .. } => Err(ui_action_error(
                "result_unknown:ui_action_accept_unconfirmed",
            )),
        }
    }

    /// Mark a previously accepted action Applied exactly once after the effect receipt exists.
    pub async fn apply_ui_action(
        &self,
        action: &UiActionCommand,
        receipt_digest: &str,
        now_unix_ms: u64,
    ) -> Result<UiActionRecord, CoreError> {
        let events = read_ui_action_stream(self.events.as_ref(), &action.idempotency_key).await?;
        let current =
            latest_record(&events)?.ok_or_else(|| ui_action_error("ui_action_not_accepted"))?;
        if current.command_digest != action.command_digest {
            return Err(ui_action_error("ui_action_idempotency_digest_mismatch"));
        }
        let next = current
            .apply_transition(receipt_digest.to_owned(), now_unix_ms)
            .map_err(|error| ui_action_error(format!("ui_action_apply_rejected:{error}")))?;
        if next.state == UiActionState::Applied && current.state == UiActionState::Applied {
            return Ok(current);
        }
        let outcome = commit_ui_action_event(
            self.events.as_ref(),
            action,
            &next,
            UI_ACTION_APPLIED_KIND,
            events.len() as u64 + 1,
            derived_request_id("ui.action.apply", &action.command_id.to_string()),
            "applied",
        )
        .await?;
        match outcome {
            CommitOutcome::Committed { .. } => Ok(next),
            CommitOutcome::Replayed { .. } => self
                .read_original_record(&action.idempotency_key)
                .await
                .map(|value| value.ok_or_else(|| ui_action_error("ui_action_record_missing")))?,
            CommitOutcome::Conflict { .. } => Err(ui_action_error("ui_action_cas_conflict")),
            CommitOutcome::Unknown { .. } => Err(ui_action_error(
                "result_unknown:ui_action_apply_unconfirmed",
            )),
        }
    }

    async fn finish_ui_action(
        &self,
        action: &UiActionCommand,
        reason: &str,
        unknown: bool,
    ) -> Result<UiActionRecord, CoreError> {
        let events = read_ui_action_stream(self.events.as_ref(), &action.idempotency_key).await?;
        let current =
            latest_record(&events)?.ok_or_else(|| ui_action_error("ui_action_not_accepted"))?;
        if current.command_digest != action.command_digest {
            return Err(ui_action_error("ui_action_idempotency_digest_mismatch"));
        }
        let next = if unknown {
            current.unknown_transition(reason.to_owned())
        } else {
            current.reject_transition(reason.to_owned())
        }
        .map_err(|error| ui_action_error(format!("ui_action_terminal_rejected:{error}")))?;
        if next.state == current.state {
            return Ok(current);
        }
        let kind = if unknown {
            UI_ACTION_UNKNOWN_KIND
        } else {
            UI_ACTION_REJECTED_KIND
        };
        let suffix = if unknown { "unknown" } else { "rejected" };
        let outcome = commit_ui_action_event(
            self.events.as_ref(),
            action,
            &next,
            kind,
            events.len() as u64 + 1,
            derived_request_id(
                &format!("ui.action.{suffix}"),
                &action.command_id.to_string(),
            ),
            suffix,
        )
        .await?;
        match outcome {
            CommitOutcome::Committed { .. } => Ok(next),
            CommitOutcome::Replayed { .. } => self
                .read_original_record(&action.idempotency_key)
                .await
                .map(|value| value.ok_or_else(|| ui_action_error("ui_action_record_missing")))?,
            CommitOutcome::Conflict { .. } => Err(ui_action_error("ui_action_cas_conflict")),
            CommitOutcome::Unknown { .. } => Err(ui_action_error(
                "result_unknown:ui_action_terminal_unconfirmed",
            )),
        }
    }

    pub async fn reject_ui_action(
        &self,
        action: &UiActionCommand,
        reason: &str,
    ) -> Result<UiActionRecord, CoreError> {
        self.finish_ui_action(action, reason, false).await
    }

    pub async fn mark_ui_action_unknown(
        &self,
        action: &UiActionCommand,
        reason: &str,
    ) -> Result<UiActionRecord, CoreError> {
        self.finish_ui_action(action, reason, true).await
    }

    /// Read the durable record for one idempotency key without an owner/scope check.
    ///
    /// 【调用者】facade 自身的对账路径：admit/apply/terminal 提交返回 Replayed 时用它取回原结果。
    /// 【输入/输出】幂等键形状校验后读取该聚合流并解码最新记录；没有记录时返回 None。
    /// 【失败】键非法、读取失败或记录载荷无法解码/校验时返回结构化错误。
    /// 【边界】本方法不做 caller/owner/scope 过滤，因此**不是**面向 surface 的查询入口；
    /// 对外查询必须走 `query_original_ui_action_for()`。
    async fn read_original_record(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<UiActionRecord>, CoreError> {
        if idempotency_key.trim().is_empty() || idempotency_key.len() > 256 {
            return Err(ui_action_error("ui_action_idempotency_key_invalid"));
        }
        let events = read_ui_action_stream(self.events.as_ref(), idempotency_key).await?;
        latest_record(&events)
    }

    /// Reconcile a lost ACK using the original idempotency key; a new command ID is never used.
    ///
    /// 【调用者】入口/工作台在响应丢失后按原 command ID/idempotency key 对账。
    /// 【输入】原幂等键、服务端已认证的 owner 标识和本次操作所在 scope 的摘要。
    /// 【输出】键、owner、scope 三者都匹配时返回持久记录（含其重投策略依据的状态）；键不存在时返回 None。
    /// 【失败】owner/scope 参数形状非法、或记录属于另一个 owner/scope 时返回稳定错误，且不返回记录内容。
    /// 【为什么需要 owner/scope】只有幂等键就能读到任意记录会让一个 surface 通过猜测键读取别人的操作；
    /// 这里要求调用方用服务端身份和 scope 摘要证明自己就是该记录的准入主体。
    /// 【边界】owner/scope 由调用方传入，因此本方法只保证"传对了才给看"，不替代入口层的身份认证。
    pub async fn query_original_ui_action_for(
        &self,
        idempotency_key: &str,
        owner_id: &str,
        scope_digest: &str,
    ) -> Result<Option<UiActionRecord>, CoreError> {
        if owner_id.trim().is_empty() || owner_id.len() > 256 {
            return Err(ui_action_error("ui_action_query_owner_invalid"));
        }
        if !valid_digest(scope_digest) {
            return Err(ui_action_error("ui_action_query_scope_invalid"));
        }
        match self.read_original_record(idempotency_key).await? {
            Some(record) if record.owner_id != owner_id || record.scope_digest != scope_digest => {
                Err(ui_action_error("ui_action_query_scope_mismatch"))
            }
            other => Ok(other),
        }
    }
}
