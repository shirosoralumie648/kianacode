//! ER-07 Core `ReplayProjection` 重放及只读边界测试。
//!
//! 前两个用例用确定性 fold 比较从 checkpoint 接续与从零重建的结果，并检查 projector/fold 失败会拒绝恢复；
//! 最后一个 `include_str!` 用例只静态搜索 helper 的关键实现标记和禁止依赖，不是实际 EventStore 路由测试。

use kiana_core::ReplayProjection;
use kiana_domain::{ProjectionCheckpoint, RequestId, RuntimeEvent};
use serde_json::json;

/// 创建具有指定 request 内序号与 delta 的测试事件。
fn event(request_id: RequestId, sequence: u64, delta: i64) -> RuntimeEvent {
    RuntimeEvent::new(
        request_id,
        sequence,
        "projection.delta",
        json!({"delta":delta}),
    )
    .unwrap()
}

/// 纯 fold：把事件 payload 的 delta 累加到 JSON state，不访问外部资源。
fn fold(state: serde_json::Value, event: &RuntimeEvent) -> Result<serde_json::Value, String> {
    let mut state = state;
    let count = state["count"].as_i64().unwrap_or_default();
    let delta = event.data["delta"].as_i64().unwrap_or_default();
    state["count"] = json!(count + delta);
    Ok(state)
}

/// 验证 checkpoint tail replay 与完整从零重放得到同一 state 和 checkpoint 摘要。
///
/// tail 故意重复包含 checkpoint 已应用过的第二个事件，检查 helper 按 EventId 去重后不会再次累加。
#[test]
fn replay_from_checkpoint_matches_rebuild_and_skips_duplicate_event_ids() {
    let request_id = RequestId::new();
    let first = event(request_id, 1, 2);
    let second = event(request_id, 2, 3);
    let third = event(request_id, 3, 5);
    let zero = ReplayProjection::from_zero(
        "counter",
        json!({"count":0}),
        &[first.clone(), second.clone()],
        2,
        fold,
    )
    .unwrap();
    let checkpoint = zero.checkpoint().unwrap();
    let resumed = ReplayProjection::from_checkpoint(
        "counter",
        &checkpoint,
        &[second.clone(), third.clone()],
        3,
        fold,
    )
    .unwrap();
    let rebuilt = ReplayProjection::from_zero(
        "counter",
        json!({"count":0}),
        &[first, second, third],
        3,
        fold,
    )
    .unwrap();
    assert_eq!(resumed.state(), rebuilt.state());
    assert_eq!(resumed.state_digest(), rebuilt.state_digest());
    assert_eq!(
        resumed.checkpoint().unwrap().checkpoint_digest,
        rebuilt.checkpoint().unwrap().checkpoint_digest
    );
}

/// 验证 projector 身份不匹配和 fold 错误都不能返回可用 projection。
///
/// 后面的静态 guard 还检查 helper 不依赖 capability/执行许可，但这些 fixture 不会触碰真实 journal。
#[test]
fn replay_checkpoint_failure_is_fail_closed_and_cannot_authorize() {
    let request_id = RequestId::new();
    let event = event(request_id, 1, 1);
    let checkpoint =
        ProjectionCheckpoint::new("counter", 0, Vec::new(), json!({"count":0})).unwrap();
    assert!(
        ReplayProjection::from_checkpoint("different", &checkpoint, &[event.clone()], 1, fold,)
            .is_err()
    );
    assert!(ReplayProjection::from_checkpoint(
        "counter",
        &checkpoint,
        &[event],
        0,
        |_state, _event| Err("source_failure".to_owned()),
    )
    .is_err());
}

/// 源文本守卫：确保 replay/checkpoint/事件 ID 上限等关键边界仍留在 helper 内。
///
/// `include_str!` 把源码当字符串检查；禁止词清单有限，因此它不能替代运行时调用链审查。
#[test]
fn er07_replay_helper_is_read_only_and_checkpoint_driven() {
    let helper = include_str!("../src/projection_checkpoint.rs");
    let domain = include_str!("../../kiana-domain/src/projection_contracts.rs");
    for marker in [
        "from_zero",
        "from_checkpoint",
        "ProjectionCheckpoint",
        "source_cursor",
        "source_event_ids",
        "projection_checkpoint_digest_mismatch",
        "projection_fold_failed",
        "BTreeSet",
        "MAX_SOURCE_EVENT_IDS",
    ] {
        assert!(
            helper.contains(marker) || domain.contains(marker),
            "ER-07 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBrokerPort",
        "ExecutionPermit",
        "commit_transition",
        "append_event",
        "signal_cancel",
    ] {
        assert!(
            !helper.contains(forbidden),
            "projection helper must not {forbidden}"
        );
    }
}
