use kiana_core::UiActionAuthoritySnapshot;

fn valid_authority() -> UiActionAuthoritySnapshot {
    UiActionAuthoritySnapshot {
        authority_epoch: "epoch-1".to_owned(),
        cursor: 7,
        target_revision: Some(3),
        owner_id: "owner-1".to_owned(),
        scope_digest: format!("sha256:{}", "a".repeat(64)),
        permit_digest: Some(format!("sha256:{}", "b".repeat(64))),
        cancelled: false,
    }
}

#[test]
fn authority_rejects_non_hex_permit_digest_before_admission() {
    let mut authority = valid_authority();
    authority.permit_digest = Some(format!("sha256:{}", "g".repeat(64)));

    assert_eq!(
        authority.validate().unwrap_err(),
        "ui_action_permit_digest_invalid"
    );
}
