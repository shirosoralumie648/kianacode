use kiana_domain::{
    merge_memory_deletion_entries, DataClass, DataPolicy, EventId, MemoryAdmission,
    MemoryDeletionEntry, MemoryDeletionIndex, MemoryEvidence, MemoryOrigin,
    MemoryProjectionDisposition, MemoryProjectionFence, MemoryRecord, MemoryRetrievalSurface,
    MemorySensitivity, MemoryState, ProcessingGrant, Purpose, RequestId, Retention,
};

fn grant(expiry: Option<u64>) -> ProcessingGrant {
    ProcessingGrant {
        id: "grant-1".to_owned(),
        source_path: "src/input.txt".to_owned(),
        content_hash: format!("sha256:{}", "a".repeat(64)),
        class: DataClass::Internal,
        purpose: Purpose {
            id: "memory.search".to_owned(),
            description: "PD-18 projection".to_owned(),
        },
        retention: Retention {
            expires_at_ms: expiry,
            retain_audit_metadata: true,
        },
        parent_ids: Vec::new(),
        created_by: "local-user".to_owned(),
        revoked: false,
    }
}

fn row_digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn active_record(project_root: &str) -> MemoryRecord {
    MemoryRecord {
        project_root: project_root.to_owned(),
        schema: kiana_domain::MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: "memory-1".to_owned(),
        layer: "project".to_owned(),
        collection: "project:code".to_owned(),
        text: "approved fact".to_owned(),
        source: "src/input.txt".to_owned(),
        role_id: "builder".to_owned(),
        department_id: "executing".to_owned(),
        session_id: "pd18-session".to_owned(),
        created_at_ms: 1,
        kind: "fact".to_owned(),
        origin: MemoryOrigin::Model,
        admission_state: MemoryAdmission::Qualified,
        state: MemoryState::Active,
        classification: kiana_domain::MemoryClassification::Project,
        purpose: Some(Purpose {
            id: "memory.review".to_owned(),
            description: "approved fact".to_owned(),
        }),
        sensitivity: MemorySensitivity::Internal,
        evidence: vec![MemoryEvidence {
            event_id: EventId::new(),
            request_id: RequestId::new(),
            run_id: None,
            quote: "approved".to_owned(),
        }],
        reviewed_by: Some("local-user".to_owned()),
        reviewed_at_ms: Some(2),
        // 【为什么必须显式给 revision】
        // `MemoryRecord` 是 `#[derive(Default)]`，所以 `MemoryRecord::default().revision` 是 0；
        // 而同一个字段上挂着 `#[serde(default = "first_revision")]`，`first_revision()` 返回 1。
        // 于是「从 JSON 还原、缺 revision」得到 1，「用 Default 构造」得到 0——同一字段两套默认值。
        // `MemoryProjectionFence::validate` 要求 `record_revision != 0`（revision 0 意味着
        // 记录尚未持久化，不该有投影栅栏），所以沿用 `..MemoryRecord::default()` 的夹具
        // 造出的记录永远过不了栅栏，三个用例全部报 `memory_projection_fence_invalid`。
        // 这里显式给出 1，与 serde 侧的 `first_revision()` 对齐。
        revision: 1,
        ..MemoryRecord::default()
    }
}

fn deletion_entry() -> MemoryDeletionEntry {
    MemoryDeletionEntry::new("memory-1", "project:code", 1, row_digest('b'), 5).unwrap()
}

fn deletion_index(policy: &DataPolicy, entry: MemoryDeletionEntry) -> MemoryDeletionIndex {
    MemoryDeletionIndex::new("/repo", policy.revision, policy.data_epoch, 10, vec![entry]).unwrap()
}

#[test]
fn active_memory_is_fenced_by_epoch_scope_and_policy() {
    let mut policy = DataPolicy::default();
    policy.register(grant(None)).unwrap();
    let fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        None,
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap();
    assert_eq!(fence.disposition, MemoryProjectionDisposition::Searchable);
    assert_eq!(fence.reason, "allowed");
    assert!(fence.deletion_index_digest.is_none());
}

#[test]
fn candidate_project_mismatch_revocation_and_retention_never_search() {
    let mut policy = DataPolicy::default();
    policy.register(grant(Some(10))).unwrap();

    let mut candidate = active_record("/repo");
    candidate.admission_state = MemoryAdmission::Candidate;
    candidate.state = MemoryState::Draft;
    candidate.reviewed_by = None;
    candidate.reviewed_at_ms = None;
    let candidate_fence = MemoryProjectionFence::evaluate(&candidate, &policy, None, 1, "/repo", 1);
    assert_eq!(candidate_fence.unwrap().reason, "candidate_not_qualified");

    let project_fence = MemoryProjectionFence::evaluate(
        &active_record("/repo-a"),
        &policy,
        None,
        policy.data_epoch,
        "/repo-b",
        1,
    )
    .unwrap();
    assert_eq!(project_fence.reason, "project_scope_denied");

    let retention_fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        None,
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap();
    assert_eq!(
        retention_fence.reason,
        "source_revoked_or_retention_expired"
    );

    policy.revoke("grant-1").unwrap();
    let revoked_fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        None,
        policy.data_epoch,
        "/repo",
        1,
    )
    .unwrap();
    assert_eq!(revoked_fence.reason, "source_revoked_or_retention_expired");
}

#[test]
fn stale_epoch_is_visible_and_denies_projection() {
    let mut policy = DataPolicy::default();
    policy.register(grant(None)).unwrap();
    policy.revoke("grant-1").unwrap();
    let fence =
        MemoryProjectionFence::evaluate(&active_record("/repo"), &policy, None, 1, "/repo", 1)
            .unwrap();
    assert_eq!(fence.disposition, MemoryProjectionDisposition::Denied);
    assert_eq!(fence.reason, "data_epoch_stale");
    assert_eq!(
        MemoryProjectionFence::validate_epoch(policy.data_epoch, 1).unwrap_err(),
        "memory_projection_data_epoch_mismatch"
    );
}

#[test]
fn record_retention_expiry_denies_a_perfectly_qualified_row() {
    let mut policy = DataPolicy::default();
    policy.register(grant(None)).unwrap();

    let mut expired = active_record("/repo");
    expired.retention = Some(Retention {
        expires_at_ms: Some(5),
        retain_audit_metadata: true,
    });
    let fence =
        MemoryProjectionFence::evaluate(&expired, &policy, None, policy.data_epoch, "/repo", 5)
            .unwrap();
    assert_eq!(fence.disposition, MemoryProjectionDisposition::Denied);
    assert_eq!(fence.reason, "retention_expired");

    let mut live = active_record("/repo");
    live.retention = Some(Retention {
        expires_at_ms: Some(50),
        retain_audit_metadata: true,
    });
    let fence =
        MemoryProjectionFence::evaluate(&live, &policy, None, policy.data_epoch, "/repo", 10)
            .unwrap();
    assert_eq!(fence.reason, "allowed");
}

#[test]
fn deletion_index_denies_revived_rows_until_every_surface_has_evidence() {
    let mut policy = DataPolicy::default();
    policy.register(grant(None)).unwrap();

    // 尚未收到任何面回执：读取被拒，并且理由点名是哪一个面还能复活这条记录。
    let mut entry = deletion_entry();
    let index = deletion_index(&policy, entry.clone());
    let fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        Some(&index),
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap();
    assert_eq!(fence.disposition, MemoryProjectionDisposition::Denied);
    assert_eq!(
        fence.reason,
        "memory_deletion_tombstone_revival_denied:lexical"
    );
    assert_eq!(
        fence.deletion_index_digest.as_deref(),
        Some(index.index_digest.as_str())
    );

    // 四个检索面各自给出证据后，索引成为“已完成失效”，理由收敛为 tombstone 本身。
    for surface in MemoryRetrievalSurface::ALL {
        entry.fence_surface(surface, 6, row_digest('c')).unwrap();
    }
    assert!(entry.fully_invalidated());
    let fenced_index = deletion_index(&policy, entry);
    assert!(fenced_index.pending_surfaces().is_empty());
    let fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        Some(&fenced_index),
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap();
    assert_eq!(fence.reason, "memory_deletion_tombstoned");
    assert_ne!(
        fenced_index.index_digest, index.index_digest,
        "回执必须改变索引摘要，否则无法归因"
    );
}

#[test]
fn deletion_index_covers_only_deleted_revisions_and_survives_merge() {
    let mut policy = DataPolicy::default();
    policy.register(grant(None)).unwrap();
    let entry = deletion_entry();
    let index = deletion_index(&policy, entry.clone());
    assert!(index.entry_covering("memory-1", 1).is_some());
    assert!(index.check_read("memory-1", 1).is_err());
    // 删除之后再写入的新 revision 不是被删对象，不能被旧 tombstone 覆盖。
    assert!(index.entry_covering("memory-1", 2).is_none());
    assert!(index.check_read("memory-1", 2).is_ok());

    // 合并必须保留回执（证据更多的一侧胜出），否则重读旧索引会复活记录。
    let mut fenced = entry.clone();
    for surface in MemoryRetrievalSurface::ALL {
        fenced.fence_surface(surface, 6, row_digest('c')).unwrap();
    }
    let merged = merge_memory_deletion_entries(&[entry.clone()], &[fenced.clone()]);
    assert_eq!(merged.len(), 1);
    assert!(merged[0].fully_invalidated());
    let merged_backwards = merge_memory_deletion_entries(&[fenced], &[entry]);
    assert_eq!(merged_backwards.len(), 1);
    assert!(merged_backwards[0].fully_invalidated());
}

#[test]
fn stale_foreign_or_tampered_deletion_index_never_serves() {
    let mut policy = DataPolicy::default();
    policy.register(grant(None)).unwrap();

    let stale = MemoryDeletionIndex::new(
        "/repo",
        policy.revision,
        policy.data_epoch + 1,
        10,
        vec![deletion_entry()],
    )
    .unwrap();
    let fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        Some(&stale),
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap();
    assert_eq!(fence.disposition, MemoryProjectionDisposition::Denied);
    assert_eq!(fence.reason, "memory_deletion_index_epoch_stale");

    let ahead = MemoryDeletionIndex::new(
        "/repo",
        policy.revision + 1,
        policy.data_epoch,
        10,
        vec![deletion_entry()],
    )
    .unwrap();
    let fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        Some(&ahead),
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap();
    assert_eq!(fence.reason, "memory_deletion_index_policy_ahead");

    let foreign = MemoryDeletionIndex::new(
        "/elsewhere",
        policy.revision,
        policy.data_epoch,
        10,
        vec![deletion_entry()],
    )
    .unwrap();
    let fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        Some(&foreign),
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap();
    assert_eq!(fence.reason, "deletion_index_project_mismatch");

    let mut tampered = deletion_index(&policy, deletion_entry());
    tampered.index_digest = row_digest('d');
    let error = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        Some(&tampered),
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap_err();
    assert_eq!(error, "memory_deletion_index_digest_mismatch");
}

#[test]
fn fenced_index_carries_evidence_bound_receipts() {
    let mut policy = DataPolicy::default();
    policy.register(grant(None)).unwrap();
    let mut entry = deletion_entry();
    // 没有证据的“已失效”声明必须被拒绝：Unknown 才是不带证据的合法状态。
    assert_eq!(
        entry.fence_surface(MemoryRetrievalSurface::Cache, 0, row_digest('c')),
        Err("memory_deletion_surface_observed_at_required".to_owned())
    );
    assert_eq!(
        entry.fence_surface(MemoryRetrievalSurface::Cache, 6, "sha256:short"),
        Err("memory_deletion_surface_evidence_digest_invalid".to_owned())
    );
    assert_eq!(
        MemoryDeletionEntry::new("memory-1", "project:code", 0, row_digest('b'), 5).unwrap_err(),
        "memory_deletion_entry_boundary_invalid".to_owned()
    );

    entry
        .fence_surface(MemoryRetrievalSurface::Cache, 6, row_digest('c'))
        .unwrap();
    let index = deletion_index(&policy, entry);
    assert_eq!(
        index.pending_surfaces(),
        vec!["lexical", "vector", "receipt"]
    );
    assert!(index.validate().is_ok());
    assert_eq!(index.tombstones(), 1);
}
