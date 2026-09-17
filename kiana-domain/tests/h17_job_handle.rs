use kiana_domain::{InvocationId, JobHandle, RequestId, RunId, SessionId, TurnId};

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

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
