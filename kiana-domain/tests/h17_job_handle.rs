//! H17 的 Domain 层 JobHandle 合同测试。
//!
//! 生产数据由 Daemon 在长进程启动后创建；本文件只用合成 ID、owner 和摘要验证 DTO 的
//! 序列化往返、Invocation 派生关系与字段篡改检测，不启动 OS 进程，也不证明 Daemon 的
//! 授权、过期或重启处理实际运行。
use kiana_domain::{InvocationId, JobHandle, RequestId, RunId, SessionId, TurnId};

// 测试只需要满足 `sha256:` + 64 位 hex 的合同外形；这些固定值不是生产内容摘要或密钥。
fn digest(ch: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (ch as u32) & 0xffff).repeat(16)
    )
}

/// 构造后确认启动 Invocation 可从原 request ID 重现，且 Serde JSON 往返不丢字段。
#[test]
fn job_handle_round_trip_binds_start_invocation_and_process_group() {
    let job_id = RequestId::new();
    let start_request_id = RequestId::new();
    let handle = JobHandle::new(
        job_id,
        start_request_id,
        Some(RunId::new()),
        Some(TurnId::new()),
        "owner",
        SessionId::new("session"),
        digest('p'),
        3,
        Some(1234),
        9_999,
    )
    .unwrap();
    assert_eq!(
        handle.start_invocation_id,
        InvocationId::from_uuid(start_request_id.as_uuid())
    );
    handle.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<JobHandle>(serde_json::to_value(&handle).unwrap()).unwrap(),
        handle
    );
}

/// 改写进程组证据而不更新摘要时必须失败；这只验证完整性检查，不表示摘要可认证来源。
#[test]
fn job_handle_digest_detects_pid_or_scope_tamper() {
    let job_id = RequestId::new();
    let start_request_id = RequestId::new();
    let mut handle = JobHandle::new(
        job_id,
        start_request_id,
        None,
        None,
        "owner",
        SessionId::new("session"),
        digest('p'),
        3,
        Some(1234),
        9_999,
    )
    .unwrap();
    handle.process_group_id = Some(5678);
    assert_eq!(handle.validate().unwrap_err(), "job_handle_digest_mismatch");
}

/// 进程组证据、authority epoch 或到期时刻为零/缺失时，构造期就必须拒绝；
/// 「只有 PID」或「永久有效」的形状不能成为合法句柄。
#[test]
fn job_handle_requires_process_group_epoch_and_expiry() {
    let session = SessionId::new("session");
    let project_digest = digest('p');
    let build = |process_group_id: Option<u32>, authority_epoch: u64, expires_at_unix_ms: u64| {
        JobHandle::new(
            RequestId::new(),
            RequestId::new(),
            None,
            None,
            "owner",
            session.clone(),
            project_digest.clone(),
            authority_epoch,
            process_group_id,
            expires_at_unix_ms,
        )
    };
    assert_eq!(
        build(None, 1, 9_999).unwrap_err(),
        "job_handle_header_invalid"
    );
    assert_eq!(
        build(Some(0), 1, 9_999).unwrap_err(),
        "job_handle_header_invalid"
    );
    assert_eq!(
        build(Some(4242), 0, 9_999).unwrap_err(),
        "job_handle_header_invalid"
    );
    assert_eq!(
        build(Some(4242), 1, 0).unwrap_err(),
        "job_handle_header_invalid"
    );
    assert!(build(Some(4242), 1, 9_999).is_ok());
}
