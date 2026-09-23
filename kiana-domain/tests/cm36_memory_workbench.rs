use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, EvidenceStatus, MemoryAccessPath, MemoryAclRequest,
    MemoryAdmission, MemoryBulkAtomicity, MemoryBulkFailure, MemoryBulkItemStatus,
    MemoryBulkMutationItem, MemoryBulkMutationItemResult, MemoryBulkMutationPlan,
    MemoryBulkMutationResult, MemoryBulkResultState, MemoryClassification, MemoryCollection,
    MemoryExportManifest, MemoryExportRow, MemoryMutation, MemoryMutationAuthority,
    MemoryMutationOperation, MemoryMutationTarget, MemoryOrigin, MemoryRecord, MemoryScope,
    MemorySensitivity, MemoryState, MemoryValidity, MemoryWorkbenchList, MemoryWorkbenchRelations,
    ProjectIdentity, Purpose, SourceKind, SourceRef,
};
use serde_json::json;
use std::collections::BTreeMap;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

fn scope(collections: &[&str]) -> MemoryScope {
    MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        ProjectIdentity::new(
            "/tmp/cm36-project",
            "/tmp/cm36-project",
            Some(1),
            Some(2),
            "trust",
        )
        .unwrap(),
        "cm36-session",
        collections
            .iter()
            .map(|value| MemoryCollection::parse(value).unwrap())
            .collect(),
        Purpose {
            id: "memory.review".to_owned(),
            description: "CM-36 review fixture".to_owned(),
        },
        true,
    )
    .unwrap()
}

fn record(id: &str, collection: &str, text: &str) -> MemoryRecord {
    let collection = MemoryCollection::parse(collection).unwrap();
    let record = MemoryRecord {
        project_root: "/tmp/cm36-project".to_owned(),
        schema: "kiana.memory-record.v2".to_owned(),
        id: id.to_owned(),
        layer: collection.layer.clone(),
        collection: collection.collection.clone(),
        text: text.to_owned(),
        source: "event:cm36".to_owned(),
        role_id: String::new(),
        department_id: String::new(),
        session_id: "cm36-session".to_owned(),
        created_at_ms: 1,
        kind: "fact".to_owned(),
        origin: MemoryOrigin::User,
        admission_state: MemoryAdmission::Candidate,
        state: MemoryState::Draft,
        classification: MemoryClassification::for_collection(&collection),
        purpose: Some(Purpose {
            id: "memory.review".to_owned(),
            description: "CM-36 review fixture".to_owned(),
        }),
        sensitivity: MemorySensitivity::Internal,
        validity: MemoryValidity::default(),
        revision: 1,
        ..MemoryRecord::default()
    };
    record.validate_lifecycle().unwrap();
    record
}

fn review_request(scope: MemoryScope) -> MemoryAclRequest {
    MemoryAclRequest::new(
        scope,
        MemoryAccessPath::ReviewList,
        MemorySensitivity::Restricted,
        100,
        2,
    )
    .unwrap()
}

fn evidence() -> Vec<SourceRef> {
    vec![SourceRef::new(
        "event:cm36",
        SourceKind::Event,
        "event://cm36",
        "cursor:1",
        digest("evidence"),
        Some(1),
        EvidenceStatus::Attributed,
    )
    .unwrap()]
}

fn mutation(id: &str, collection: &str, operation: MemoryMutationOperation) -> MemoryMutation {
    let mutation = MemoryMutation::new(
        format!("mutation:{id}"),
        operation,
        "local-user",
        scope(&[collection]),
        vec![MemoryMutationTarget::new(id, collection, 1)],
        evidence(),
        1,
        2,
        format!("cm36:{id}"),
        &json!({"record_id": id, "operation": operation}),
    )
    .unwrap();
    if matches!(
        operation,
        MemoryMutationOperation::Approve
            | MemoryMutationOperation::Publish
            | MemoryMutationOperation::Expire
            | MemoryMutationOperation::Delete
            | MemoryMutationOperation::Revoke
    ) {
        mutation
            .with_authority(MemoryMutationAuthority::Human)
            .unwrap()
    } else {
        mutation
    }
}

fn item(
    id: &str,
    collection: &str,
    group: &str,
    operation: MemoryMutationOperation,
) -> MemoryBulkMutationItem {
    MemoryBulkMutationItem {
        group_id: group.to_owned(),
        mutation: mutation(id, collection, operation),
    }
}

#[test]
fn private_memory_is_redacted_in_list_and_export() {
    let records = vec![
        record("public-1", "project:code", r#"{"token":"sk-secret"}"#),
        record("private-1", "user:prefs", "my private preference"),
    ];
    let mut relations = BTreeMap::new();
    relations.insert(
        "public-1".to_owned(),
        MemoryWorkbenchRelations {
            similar_record_ids: vec!["private-1".to_owned(), "outside-scope".to_owned()],
            conflict_set_id: Some("conflict:one".to_owned()),
        },
    );
    relations.insert(
        "private-1".to_owned(),
        MemoryWorkbenchRelations {
            similar_record_ids: vec!["public-1".to_owned()],
            conflict_set_id: Some("conflict:one".to_owned()),
        },
    );
    let list = MemoryWorkbenchList::from_records(
        "list:cm36",
        &review_request(scope(&["project:code", "user:prefs"])),
        "memory review",
        &records,
        &relations,
        None,
    )
    .unwrap();
    let private = list
        .entries
        .iter()
        .find(|entry| entry.record_id == "private-1")
        .unwrap();
    assert!(private.preview.is_none());
    assert!(private.preview_redacted);
    let public = list
        .entries
        .iter()
        .find(|entry| entry.record_id == "public-1")
        .unwrap();
    assert_eq!(public.similar_record_ids, vec!["private-1"]);
    assert!(!public.preview.as_deref().unwrap().contains("sk-secret"));

    let export = MemoryExportManifest::from_list(
        "export:cm36",
        &list,
        digest("purpose"),
        digest("recipient"),
        digest("redaction-profile"),
    )
    .unwrap();
    let private_export = export
        .rows
        .iter()
        .find(|row| row.record_id == "private-1")
        .unwrap();
    assert!(private_export.preview.is_none());
    assert!(private_export.private_redacted);

    let mut forged_export = MemoryExportRow {
        record_id: "forged-private".to_owned(),
        collection: "user:prefs".to_owned(),
        classification: MemoryClassification::UserPrivate,
        revision: 1,
        content_digest: digest("private"),
        preview: None,
        private_redacted: false,
        row_digest: digest("forged-row"),
    };
    forged_export.row_digest = json_digest(&json!({
        "record_id": forged_export.record_id,
        "collection": forged_export.collection,
        "classification": forged_export.classification,
        "revision": forged_export.revision,
        "content_digest": forged_export.content_digest,
        "preview": forged_export.preview,
        "private_redacted": forged_export.private_redacted,
    }));
    assert_eq!(
        forged_export.validate().unwrap_err(),
        "memory_export_private_preview_leak"
    );

    let mut leaked = private.clone();
    leaked.preview = Some("private value".to_owned());
    leaked.item_digest = leaked.digest();
    assert_eq!(
        leaked.validate().unwrap_err(),
        "memory_workbench_private_preview_leak"
    );
}

#[test]
fn denied_records_and_out_of_list_relations_are_not_exposed() {
    let foreign = record("foreign", "project:code", "foreign project fact");
    let mut foreign = foreign;
    foreign.project_root = "/tmp/foreign-project".to_owned();
    let list = MemoryWorkbenchList::from_records(
        "list:denied",
        &review_request(scope(&["project:code"])),
        "review",
        &[foreign],
        &BTreeMap::new(),
        None,
    )
    .unwrap();
    assert!(list.entries.is_empty());

    let record = record("visible", "project:code", "visible fact");
    let relations = BTreeMap::from([(
        "visible".to_owned(),
        MemoryWorkbenchRelations {
            similar_record_ids: vec!["secret-outside-list".to_owned()],
            conflict_set_id: None,
        },
    )]);
    let list = MemoryWorkbenchList::from_records(
        "list:relations",
        &review_request(scope(&["project:code"])),
        "review",
        &[record],
        &relations,
        None,
    )
    .unwrap();
    assert!(list.entries[0].similar_record_ids.is_empty());
}

#[test]
fn bulk_review_rejects_partial_atomic_and_split_groups_but_reports_split_failures() {
    let atomic = MemoryBulkMutationPlan::new(
        "plan:atomic",
        "local-user",
        MemoryBulkAtomicity::Atomic,
        2,
        vec![
            item(
                "one",
                "project:code",
                "same-scope",
                MemoryMutationOperation::Approve,
            ),
            item(
                "two",
                "project:code",
                "same-scope",
                MemoryMutationOperation::Revoke,
            ),
        ],
    )
    .unwrap();
    let partial = MemoryBulkMutationResult::from_items(
        &atomic,
        vec![
            MemoryBulkMutationItemResult::committed(&atomic.items[0], digest("receipt")).unwrap(),
            MemoryBulkMutationItemResult::failed(
                &atomic.items[1],
                MemoryBulkFailure::RevisionConflict,
            )
            .unwrap(),
        ],
    );
    assert_eq!(partial.unwrap_err(), "memory_bulk_atomic_partial_result");

    let split_same_group = MemoryBulkMutationPlan::new(
        "plan:split-same-group",
        "local-user",
        MemoryBulkAtomicity::ExplicitSplit,
        2,
        vec![
            item(
                "three",
                "project:code",
                "group-a",
                MemoryMutationOperation::Publish,
            ),
            item(
                "four",
                "project:code",
                "group-a",
                MemoryMutationOperation::Expire,
            ),
        ],
    )
    .unwrap();
    let mixed_group = MemoryBulkMutationResult::from_items(
        &split_same_group,
        vec![
            MemoryBulkMutationItemResult::committed(
                &split_same_group.items[0],
                digest("receipt-2"),
            )
            .unwrap(),
            MemoryBulkMutationItemResult::unknown(&split_same_group.items[1]).unwrap(),
        ],
    );
    assert_eq!(
        mixed_group.unwrap_err(),
        "memory_bulk_split_group_partial_result"
    );

    let split = MemoryBulkMutationPlan::new(
        "plan:split",
        "local-user",
        MemoryBulkAtomicity::ExplicitSplit,
        2,
        vec![
            item(
                "five",
                "project:code",
                "group-ok",
                MemoryMutationOperation::Delete,
            ),
            item(
                "six",
                "company",
                "group-denied",
                MemoryMutationOperation::Delete,
            ),
        ],
    )
    .unwrap();
    let result = MemoryBulkMutationResult::from_items(
        &split,
        vec![
            MemoryBulkMutationItemResult::committed(&split.items[0], digest("receipt-3")).unwrap(),
            MemoryBulkMutationItemResult::failed(&split.items[1], MemoryBulkFailure::ScopeDenied)
                .unwrap(),
        ],
    )
    .unwrap();
    assert_eq!(result.state, MemoryBulkResultState::PartiallyFailed);
    assert!(result.items.iter().any(|item| {
        item.status == MemoryBulkItemStatus::Failed
            && item.failure == Some(MemoryBulkFailure::ScopeDenied)
    }));
    result.validate_against(&split).unwrap();
}

#[test]
fn bulk_plan_requires_current_target_revisions_and_reports_unknown_results() {
    let mut unapproved = item(
        "unapproved",
        "project:code",
        "group",
        MemoryMutationOperation::Approve,
    );
    unapproved.mutation.authority = MemoryMutationAuthority::Agent;
    assert_eq!(
        MemoryBulkMutationPlan::new(
            "plan:unapproved",
            "local-user",
            MemoryBulkAtomicity::Atomic,
            2,
            vec![unapproved],
        )
        .unwrap_err(),
        "memory_workbench_operator_authority_required"
    );

    let mut stale = item(
        "stale",
        "project:code",
        "group",
        MemoryMutationOperation::Update,
    );
    stale.mutation.expected_revisions[0].expected_revision = 0;
    assert_eq!(
        MemoryBulkMutationPlan::new(
            "plan:stale",
            "local-user",
            MemoryBulkAtomicity::Atomic,
            2,
            vec![stale],
        )
        .unwrap_err(),
        "memory_mutation_expected_revision_required"
    );

    let plan = MemoryBulkMutationPlan::new(
        "plan:unknown",
        "local-user",
        MemoryBulkAtomicity::ExplicitSplit,
        2,
        vec![item(
            "unknown",
            "project:code",
            "group",
            MemoryMutationOperation::Update,
        )],
    )
    .unwrap();
    let result = MemoryBulkMutationResult::from_items(
        &plan,
        vec![MemoryBulkMutationItemResult::unknown(&plan.items[0]).unwrap()],
    )
    .unwrap();
    assert_eq!(result.state, MemoryBulkResultState::Unknown);
    assert_eq!(
        result.items[0].failure,
        Some(MemoryBulkFailure::ResultUnknown)
    );
}

#[test]
fn export_digest_and_rows_are_bound_to_scope_and_recipient() {
    let list = MemoryWorkbenchList::from_records(
        "list:export",
        &review_request(scope(&["project:code"])),
        "review",
        &[record("exported", "project:code", "safe fact")],
        &BTreeMap::new(),
        None,
    )
    .unwrap();
    let manifest = MemoryExportManifest::from_list(
        "export:bound",
        &list,
        digest("purpose"),
        digest("recipient"),
        digest("redaction"),
    )
    .unwrap();
    assert_eq!(manifest.rows.len(), 1);
    let row = MemoryExportRow::from_list_item(&list.entries[0]).unwrap();
    assert!(row.row_digest.starts_with("sha256:"));
}
