use kiana_domain::{
    project_data_governance, DataClass, DataPayloadState, DataPolicy, EventId, ProcessingGrant,
    Purpose, Retention,
};

fn hash(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat(byte).take(64).collect::<String>()
    )
}

fn grant(
    id: &str,
    path: &str,
    expires_at_ms: Option<u64>,
    parent_ids: Vec<String>,
) -> ProcessingGrant {
    ProcessingGrant {
        id: id.to_owned(),
        source_path: path.to_owned(),
        content_hash: hash('a'),
        class: DataClass::Confidential,
        purpose: Purpose {
            id: "audit".to_owned(),
            description: "security review".to_owned(),
        },
        retention: Retention {
            expires_at_ms,
            retain_audit_metadata: true,
        },
        parent_ids,
        created_by: "principal:operator".to_owned(),
        revoked: false,
    }
}

#[test]
fn policy_epoch_and_digest_change_on_revocation_with_parent_cascade() {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("parent", "src/a.txt", None, Vec::new()))
        .unwrap();
    policy
        .register(grant("child", "src/b.txt", None, vec!["parent".to_owned()]))
        .unwrap();
    policy.validate().unwrap();
    let before_epoch = policy.data_epoch;
    let before_digest = policy.digest();
    let affected = policy.revoke("parent").unwrap();
    assert_eq!(affected, vec!["child".to_owned(), "parent".to_owned()]);
    assert!(policy.data_epoch > before_epoch);
    assert_ne!(policy.policy_digest, before_digest);
    policy.validate().unwrap();
    let encoded = serde_json::to_string(&policy).unwrap();
    assert_eq!(
        serde_json::from_str::<DataPolicy>(&encoded).unwrap(),
        policy
    );
}

#[test]
fn expired_payload_is_separate_from_retained_audit_metadata_and_propagates() {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("expired", "src/expired.txt", Some(10), Vec::new()))
        .unwrap();
    let snapshot =
        project_data_governance(&policy, "project:one", 4, vec![EventId::new()], 10).unwrap();
    snapshot.validate().unwrap();
    assert_eq!(snapshot.observations[0].payload, DataPayloadState::Expired);
    assert!(snapshot.observations[0].audit_metadata_retained);
    assert!(snapshot.audit_metadata_retained);
    assert!(snapshot
        .derived_store_states
        .values()
        .all(|state| *state == DataPayloadState::Expired));
}

#[test]
fn tampered_policy_or_snapshot_digest_is_rejected() {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("active", "src/active.txt", None, Vec::new()))
        .unwrap();
    let mut forged = policy.clone();
    forged.data_epoch += 1;
    assert_eq!(
        forged.validate().unwrap_err(),
        "data_policy_digest_mismatch"
    );
    let snapshot =
        project_data_governance(&policy, "project:one", 1, vec![EventId::new()], 0).unwrap();
    let mut forged_snapshot = snapshot.clone();
    forged_snapshot.audit_metadata_retained = false;
    assert_eq!(
        forged_snapshot.validate().unwrap_err(),
        "data_governance_snapshot_digest_mismatch"
    );
}
