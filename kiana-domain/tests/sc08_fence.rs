use kiana_domain::{json_digest, AuthorityFence, FenceTokenId, SecurityReasonCode};
use serde_json::json;

fn digest(value: &str) -> String {
    json_digest(&json!({"revision": value}))
}

#[test]
fn authority_fence_is_versioned_digest_linked_and_round_trips() {
    let fence = AuthorityFence::new(
        FenceTokenId::new(),
        "/repo",
        "sc08-session",
        2,
        3,
        digest("policy-2"),
        digest("config-2"),
        100,
        500,
    )
    .unwrap();
    assert!(fence.validate().is_ok());
    assert!(fence
        .validate_current(200, 2, 3, &digest("policy-2"), &digest("config-2"))
        .is_ok());
    assert_eq!(
        AuthorityFence::from_json(&fence.to_json().unwrap()).unwrap(),
        fence
    );

    let next = AuthorityFence::successor(
        &fence,
        3,
        4,
        digest("policy-3"),
        digest("config-3"),
        500,
        900,
    )
    .unwrap();
    assert!(next.validate_successor(&fence).is_ok());
    assert_eq!(next.sequence, fence.sequence + 1);
    assert_eq!(
        next.parent_digest.as_deref(),
        Some(fence.fence_digest.as_str())
    );
}

#[test]
fn authority_fence_rejects_epoch_session_policy_config_and_time_drift() {
    let fence = AuthorityFence::new(
        FenceTokenId::new(),
        "/repo",
        "sc08-session",
        2,
        3,
        digest("policy-2"),
        digest("config-2"),
        100,
        500,
    )
    .unwrap();
    assert_eq!(
        fence
            .validate_current(200, 1, 3, &digest("policy-2"), &digest("config-2"))
            .unwrap_err(),
        SecurityReasonCode::PolicyAuthorityEpochRollback.as_str()
    );
    assert_eq!(
        fence
            .validate_current(200, 3, 3, &digest("policy-2"), &digest("config-2"))
            .unwrap_err(),
        SecurityReasonCode::PolicyAuthorityEpochStale.as_str()
    );
    assert_eq!(
        fence
            .validate_current(200, 2, 4, &digest("policy-2"), &digest("config-2"))
            .unwrap_err(),
        SecurityReasonCode::AuthSessionGenerationStale.as_str()
    );
    assert_eq!(
        fence
            .validate_current(200, 2, 3, &digest("policy-new"), &digest("config-2"))
            .unwrap_err(),
        SecurityReasonCode::PolicyRevisionStale.as_str()
    );
    assert_eq!(
        fence
            .validate_current(200, 2, 3, &digest("policy-2"), &digest("config-new"))
            .unwrap_err(),
        SecurityReasonCode::PolicyConfigRevisionStale.as_str()
    );
    assert_eq!(
        fence
            .validate_current(500, 2, 3, &digest("policy-2"), &digest("config-2"))
            .unwrap_err(),
        SecurityReasonCode::UnknownFenceExpired.as_str()
    );
}

#[test]
fn authority_fence_rejects_invalid_parent_and_successor_rollback() {
    let fence = AuthorityFence::new(
        FenceTokenId::new(),
        "/repo",
        "sc08-session",
        2,
        3,
        digest("policy-2"),
        digest("config-2"),
        100,
        500,
    )
    .unwrap();
    assert_eq!(
        AuthorityFence::successor(
            &fence,
            1,
            3,
            digest("policy-1"),
            digest("config-1"),
            500,
            900,
        )
        .unwrap_err(),
        SecurityReasonCode::PolicyAuthorityEpochRollback.as_str()
    );
    assert_eq!(
        AuthorityFence::successor(
            &fence,
            2,
            2,
            digest("policy-2"),
            digest("config-2"),
            500,
            900,
        )
        .unwrap_err(),
        SecurityReasonCode::AuthSessionGenerationStale.as_str()
    );

    let mut malformed = fence.to_json().unwrap();
    malformed["sequence"] = json!(2);
    assert_eq!(
        AuthorityFence::from_json(&malformed).unwrap_err(),
        "authority_fence_parent_required"
    );
}
