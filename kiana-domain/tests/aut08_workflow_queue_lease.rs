//! AUT-08 Domain lease 状态机的纯内存测试。
//!
//! A-F 是重复字符组成的摘要格式样本，不是实际 WorkPacket SHA-256 计算值，也不证明授权来源。
//! 测试直接调用 Domain transition，不经过 QueueStore 原子提交、Daemon worker 或 EventLog 恢复。

use kiana_domain::{
    WorkflowQueueClaimContract, WorkflowQueueClaimStatus, WorkflowQueueEffectState,
    WorkflowQueueLease, WorkflowQueueLeaseStatus,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const E: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const F: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

/// 提供一条字段自洽的 Ready claim，供各用例创建第一代 Lease。
fn claim() -> WorkflowQueueClaimContract {
    WorkflowQueueClaimContract::new(
        "packet-1",
        A,
        Some(B.to_owned()),
        C,
        Some(D.to_owned()),
        E,
        F,
        true,
        false,
        true,
        true,
        0,
        1,
        false,
        None,
        2_000,
        1_000,
        WorkflowQueueClaimStatus::Ready,
    )
    .expect("valid claim")
}

/// 建立 worker-a 的 genesis Lease；sequence=1、fence/epoch=1，expiry 在测试时间之后。
fn lease() -> WorkflowQueueLease {
    WorkflowQueueLease::issue(&claim(), "worker-a", 1, 1, 1_000, 1_100).expect("valid lease")
}

#[test]
fn heartbeat_requires_current_owner_and_fence() {
    // owner 或 fencing token 任一不匹配，都不能把 lease 续到新版本。
    let value = lease();
    assert_eq!(
        value
            .renew("worker-b", 1, 1, 1_010, 1_100)
            .expect_err("wrong owner"),
        "workflow_queue_owner_mismatch"
    );
    assert_eq!(
        value
            .renew("worker-a", 2, 1, 1_010, 1_100)
            .expect_err("old fence"),
        "workflow_queue_fence_mismatch"
    );
}

#[test]
fn running_effect_cannot_be_reclaimed_after_expiry() {
    // lease 即使过期，只要 effect 已记录 Running，就不能推断外部动作未发生并转给另一个 worker。
    let running = lease()
        .record_effect("worker-a", 1, 1, 1_010, WorkflowQueueEffectState::Running)
        .expect("running effect");
    assert_eq!(
        running
            .reclaim("worker-b", 2, 1, 1_100, 1_200)
            .expect_err("in-flight effect"),
        "workflow_queue_reclaim_effect_in_flight"
    );
}

#[test]
fn unknown_effect_is_fenced_into_recovery() {
    // ResultUnknown 同时出现在 effect 与 lease status，保持后续恢复检查可见。
    let unknown = lease()
        .record_effect(
            "worker-a",
            1,
            1,
            1_010,
            WorkflowQueueEffectState::ResultUnknown,
        )
        .expect("unknown effect");
    assert_eq!(unknown.status, WorkflowQueueLeaseStatus::ResultUnknown);
    assert_eq!(
        unknown.effect_state,
        WorkflowQueueEffectState::ResultUnknown
    );
    assert!(unknown.validate().is_ok());
}

#[test]
fn safe_expiry_requires_a_new_monotonic_fence() {
    // 只有 NotStarted 的已过期 lease 可安全重领；新 owner 使用更大的 fence 并保留前一版本 digest。
    let next = lease()
        .reclaim("worker-b", 2, 2, 1_100, 1_200)
        .expect("safe reclaim");
    assert_eq!(next.owner_id, "worker-b");
    assert_eq!(next.fence_token, 2);
    assert_eq!(
        next.previous_lease_digest.as_deref(),
        Some(lease().lease_digest.as_str())
    );
}

#[test]
fn lease_ttl_is_bounded_even_when_constructed_without_the_store_adapter() {
    // 即便跳过 Store adapter，Domain 构造器也不能产生超过 5 分钟合同上限的租约。
    let error = WorkflowQueueLease::issue(
        &claim(),
        "worker-a",
        1,
        1,
        1_000,
        1_000 + kiana_domain::WORKFLOW_QUEUE_LEASE_TTL_MAX_MS + 1,
    )
    .expect_err("lease ttl must be bounded");
    assert_eq!(error, "workflow_queue_lease_header_invalid");
}
