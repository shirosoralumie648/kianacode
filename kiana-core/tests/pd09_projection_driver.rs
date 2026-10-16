//! PD-09 投影 driver 的状态转换与可重建性测试。
//!
//! 事件、游标和 fold 都在测试内构造；测试覆盖 checkpoint tail、显式 rebuild、错误暂停和手动 retry。
//! 它们不验证真实 EventLog 读取、checkpoint 持久化或后台调度。

use kiana_core::{ProjectionDriver, ProjectionDriverStatus};
use kiana_domain::{RequestId, RuntimeEvent};
use serde_json::json;

/// 创建一段 sequence 连续的合成事件；每条 payload 都给投影计数增加 1。
fn events(start: u64, count: u64) -> Vec<RuntimeEvent> {
    (0..count)
        .map(|offset| {
            RuntimeEvent::new(
                RequestId::new(),
                start + offset,
                "projection.add",
                json!({"n": 1}),
            )
            .unwrap()
        })
        .collect()
}

/// 检查 driver 能先保存一段投影、追加 tail，再清掉 checkpoint 并从全量事件确定性重建。
#[test]
fn projector_driver_checkpoints_tail_and_rebuilds_deterministically() {
    let first = events(1, 2);
    let tail = events(3, 1);
    let mut driver = ProjectionDriver::new("pd09").unwrap();
    driver
        .apply(json!({"total": 0}), &first, 2, |mut state, event| {
            state["total"] = json!(
                state["total"].as_u64().unwrap() + event.data.get("n").unwrap().as_u64().unwrap()
            );
            Ok(state)
        })
        .unwrap();
    assert_eq!(driver.status(), ProjectionDriverStatus::Ready);
    assert_eq!(driver.checkpoint().unwrap().source_cursor, 2);
    driver
        .apply(json!({"total": 0}), &tail, 3, |mut state, event| {
            state["total"] = json!(
                state["total"].as_u64().unwrap() + event.data.get("n").unwrap().as_u64().unwrap()
            );
            Ok(state)
        })
        .unwrap();
    assert_eq!(driver.checkpoint().unwrap().source_cursor, 3);
    driver.rebuild();
    assert_eq!(driver.status(), ProjectionDriverStatus::RebuildRequired);
    let all = [first, tail].concat();
    driver
        .apply(json!({"total": 0}), &all, 3, |mut state, event| {
            state["total"] = json!(
                state["total"].as_u64().unwrap() + event.data.get("n").unwrap().as_u64().unwrap()
            );
            Ok(state)
        })
        .unwrap();
    assert_eq!(driver.checkpoint().unwrap().state["total"], 3);
}

/// 检查 fold 失败后 driver 进入 Paused、拒绝隐式再次 apply，必须显式 retry 才能继续。
///
/// `retry()` 只恢复状态并递增手动重试计数；真正的第二次折叠由后续显式 `apply()` 完成。
#[test]
fn projector_driver_pauses_on_fold_error_and_requires_explicit_retry() {
    let mut driver = ProjectionDriver::new("pd09-error").unwrap();
    let event = events(1, 1);
    assert_eq!(
        driver
            .apply(json!({}), &event, 1, |_state, _event| Err(
                "fold_broken".to_owned()
            ))
            .unwrap_err(),
        "projection_fold_failed:fold_broken"
    );
    assert_eq!(driver.status(), ProjectionDriverStatus::Paused);
    assert_eq!(
        driver
            .apply(json!({}), &event, 1, |_state, _event| Ok(json!({})))
            .unwrap_err(),
        "projection_driver_paused"
    );
    driver.retry().unwrap();
    assert_eq!(driver.retry_count(), 1);
    driver
        .apply(json!({}), &event, 1, |_state, _event| {
            Ok(json!({"ok": true}))
        })
        .unwrap();
}
