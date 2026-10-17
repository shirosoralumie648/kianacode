//! ER-19 job-handle identity and resource-fencing contract fixtures.
//!
//! These fixtures exercise the pure domain contracts behind the daemon process-handle
//! boundary: a `JobHandle` is immutable start identity (job/start request/owner/scope/
//! process group/expiry/digest), and a `ResourceLease` fences live resources by token,
//! not by TTL. They do not start OS processes or prove live/durable effect behaviour.

use kiana_domain::{
    json_digest, FenceTokenId, InvocationId, JobHandle, RequestId, ResourceLease, RunId, SessionId,
    StorageLockId, TurnId,
};
use serde_json::json;

/// Build a valid handle whose digest binds all immutable identity material.
fn handle_for(job_id: RequestId, start_request_id: RequestId) -> JobHandle {
    JobHandle::new(
        job_id,
        start_request_id,
        Some(RunId::new()),
        Some(TurnId::new()),
        "er19-owner",
        SessionId::new("er19-session"),
        json_digest(&json!({"project": "er19"})),
        1,
        Some(4242),
        4_000_000_000_000,
    )
    .expect("valid job handle")
}

#[test]
fn er19_stale_handle_cannot_stop_new_execution() {
    let start_request_id = RequestId::new();
    let handle = handle_for(RequestId::new(), start_request_id);
    assert!(handle.validate().is_ok());
    // The start invocation id is derived from the start request, so a handle cannot be
    // re-pointed at a different start without breaking the identity chain.
    assert_eq!(
        handle.start_invocation_id,
        InvocationId::from_uuid(start_request_id.as_uuid())
    );

    // Re-pointing the job at another execution updates a field but not the digest; the
    // handle no longer validates, so the old handle cannot address the new execution.
    let mut repointed = handle.clone();
    repointed.job_id = RequestId::new();
    assert_eq!(
        repointed.validate(),
        Err("job_handle_digest_mismatch".to_owned())
    );

    // Process-group identity is mandatory; dropping it fails closed at the header.
    let mut no_group = handle.clone();
    no_group.process_group_id = None;
    assert_eq!(
        no_group.validate(),
        Err("job_handle_header_invalid".to_owned())
    );

    // A zero absolute-expiry marker is not a usable handle.
    let mut zero_expiry = handle.clone();
    zero_expiry.expires_at_unix_ms = 0;
    assert_eq!(
        zero_expiry.validate(),
        Err("job_handle_header_invalid".to_owned())
    );
}

#[test]
fn er19_lease_expiry_does_not_release_live_process() {
    let lease = ResourceLease::new(
        StorageLockId::new(),
        FenceTokenId::new(),
        "src/lib.rs",
        RunId::new(),
        None,
        "er19-session",
        4,
        100,
        1_000,
    )
    .expect("valid lease");

    // Inside the window the fence is current for the live resource.
    assert!(lease.validate_current(999, 4, lease.fence_token).is_ok());
    // At/after expiry the lease is denied, not reported as released.
    assert_eq!(
        lease.validate_current(1_000, 4, lease.fence_token),
        Err("resource_lease_expired".to_owned())
    );

    // A successor must advance the fence token; reusing the token is denied so a stale
    // owner cannot keep acting on the live resource.
    assert_eq!(
        ResourceLease::successor(&lease, lease.fence_token, 4, 1_100, 2_000),
        Err("resource_lease_fence_token_reused".to_owned())
    );
    let successor = ResourceLease::successor(&lease, FenceTokenId::new(), 4, 1_100, 2_000)
        .expect("fenced successor");
    assert_eq!(
        successor.validate_current(1_500, 4, lease.fence_token),
        Err("resource_lease_fence_mismatch".to_owned())
    );
}
