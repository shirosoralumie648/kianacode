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

fn grant(id: &str, path: &str, expires_at_ms: Option<u64>) -> ProcessingGrant {
    ProcessingGrant {
        id: id.to_owned(),
        source_path: path.to_owned(),
        content_hash: hash('a'),
        class: DataClass::Restricted,
        purpose: Purpose {
            id: "governance".to_owned(),
            description: "bounded retention review".to_owned(),
        },
        retention: Retention {
            expires_at_ms,
            retain_audit_metadata: true,
        },
        parent_ids: Vec::new(),
        created_by: "principal:operator".to_owned(),
        revoked: false,
    }
}

#[test]
fn deletion_propagates_to_memory_and_index() {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("active", "src/input.txt", None))
        .unwrap();
    let before =
        project_data_governance(&policy, "project:one", 1, vec![EventId::new()], 1).unwrap();
    assert!(before
        .derived_store_states
        .values()
        .all(|state| *state == DataPayloadState::Available));

    policy.revoke("active").unwrap();
    let after =
        project_data_governance(&policy, "project:one", 2, vec![EventId::new()], 1).unwrap();
    assert_eq!(after.observations[0].payload, DataPayloadState::Revoked);
    for store in [
        "receipt", "audit", "artifact", "memory", "index", "cache", "export",
    ] {
        assert_eq!(
            after.derived_store_states.get(store),
            Some(&DataPayloadState::Revoked),
            "revocation did not reach derived store {store}"
        );
    }
    assert!(after.audit_metadata_retained);
}

#[test]
fn retention_expiry_is_not_treated_as_revocation() {
    let mut policy = DataPolicy::default();
    policy
        .register(grant("expiring", "src/expiring.txt", Some(10)))
        .unwrap();
    let snapshot =
        project_data_governance(&policy, "project:one", 1, vec![EventId::new()], 10).unwrap();
    assert_eq!(snapshot.observations[0].payload, DataPayloadState::Expired);
    assert!(snapshot.audit_metadata_retained);
    assert!(snapshot
        .derived_store_states
        .values()
        .all(|state| *state == DataPayloadState::Expired));
}
