use kiana_entrypoints::workbench_review::{
    ArtifactPage, ArtifactViewState, ArtifactViewer, InboxCardState, InboxField, InboxScope,
    ReceiptCost, ReceiptDisplayState, ReceiptFile, ReceiptGolden, ReceiptProvenance, ReceiptUpdate,
    ReviewRefusal, ReviewRefusalStage, WorkbenchDecision, WorkbenchInbox, WorkbenchInboxCard,
    WorkbenchInboxDecision, WorkbenchReceipt, MAX_ARTIFACT_BYTES, MAX_ARTIFACT_PAGES,
    MAX_ARTIFACT_PAGE_BYTES, MAX_INBOX_CARDS, MAX_INBOX_DECISIONS, MAX_INBOX_FIELDS,
    MAX_INBOX_PAYLOAD_BYTES, MAX_RECEIPT_FILES, MAX_RECEIPT_LIMITATIONS, MAX_SCOPE_PATHS,
    REVIEW_REFUSALS, WORKBENCH_RECEIPT_GOLDEN_SCHEMA, WORKBENCH_RECEIPT_SCHEMA,
    WORKBENCH_REVIEW_SCHEMA,
};
use kiana_protocol::{
    json_digest, ArtifactId, ArtifactProvenance, ArtifactVersion, ExecutionStatus, HumanActionCard,
    RunId,
};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const RUN_UUID: &str = "12345678-1234-4234-8234-123456789abc";
const FIXTURE: &str = include_str!("fixtures/ui15-workbench-review.json");

fn fixed_run_id() -> RunId {
    RunId::parse_str(RUN_UUID).expect("the UI-15 fixture run id must stay a valid UUID")
}

fn scope() -> InboxScope {
    InboxScope {
        scope_digest: DIGEST.to_owned(),
        summary: "workspace files under src/".to_owned(),
        paths: vec!["src/main.rs".to_owned()],
    }
}

fn field(name: &str) -> InboxField {
    InboxField {
        name: name.to_owned(),
        label: "Review comment".to_owned(),
        field_type: "text".to_owned(),
        required: true,
        allowed_values: Vec::new(),
    }
}

fn card() -> WorkbenchInboxCard {
    let payload = json!({"decision":"approve", "scope":"src/"});
    WorkbenchInboxCard {
        schema: WORKBENCH_REVIEW_SCHEMA.to_owned(),
        action_id: "approval-15".to_owned(),
        command: "approval.decide".to_owned(),
        target_id: "run-15".to_owned(),
        reason: "The requested write needs operator review.".to_owned(),
        scope: scope(),
        expected_revision: Some(4),
        expires_at_unix_ms: Some(2_000),
        revoked: false,
        fields: vec![field("comment")],
        allowed_decisions: WorkbenchDecision::ALL
            .into_iter()
            .map(|decision| decision.as_str().to_owned())
            .collect(),
        payload_digest: json_digest(&payload),
        payload,
        artifact_refs: Vec::new(),
    }
}

fn artifact(content: &[u8]) -> kiana_protocol::ArtifactRef {
    ArtifactVersion::new(
        ArtifactId::new(),
        1,
        "text/x-diff",
        content,
        DIGEST,
        ArtifactProvenance {
            producer_kind: "workbench".to_owned(),
            producer_id: "ui15".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "daemon".to_owned(),
        },
        1,
    )
    .unwrap()
    .as_ref()
}

fn page_digest(reference: &kiana_protocol::ArtifactRef, content: &[u8]) -> String {
    ArtifactVersion::new(
        reference.artifact_id,
        reference.version,
        reference.artifact_schema.clone(),
        content,
        reference.scope_digest.clone(),
        reference.provenance.clone(),
        1,
    )
    .unwrap()
    .content_hash
}

fn unknown_receipt() -> WorkbenchReceipt {
    WorkbenchReceipt {
        schema: WORKBENCH_RECEIPT_SCHEMA.to_owned(),
        run_id: fixed_run_id(),
        status: ExecutionStatus::ResultUnknown,
        files: vec![ReceiptFile {
            path: "src/main.rs".to_owned(),
            status: "modified".to_owned(),
            revision: 4,
            artifact_ref: None,
        }],
        cost: ReceiptCost {
            currency: "USD".to_owned(),
            amount_micros: None,
            usage_known: false,
            cost_known: false,
        },
        unknown: true,
        limitations: vec!["provider_receipt_pending".to_owned()],
        provenance: ReceiptProvenance {
            receipt_digest: DIGEST.to_owned(),
            source_cursor: 15,
            source_event_ids: vec!["event-15".to_owned()],
            artifact_digests: vec![],
            producer: Some("daemon".to_owned()),
        },
    }
}

#[test]
fn inbox_display_keeps_reason_scope_expiry_fields_and_allowed_decisions() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fixture["schema"], "kiana.ui15-workbench-review.v1");
    assert_eq!(fixture["step"], "UI-15");
    assert_eq!(fixture["review_schema"], WORKBENCH_REVIEW_SCHEMA);
    assert_eq!(fixture["receipt_schema"], WORKBENCH_RECEIPT_SCHEMA);
    for marker in fixture["deny_first"].as_array().unwrap() {
        assert!(!marker.as_str().unwrap().is_empty());
    }
    let card = card();
    card.validate().unwrap();
    assert_eq!(
        card.reason.as_str(),
        fixture["inbox"]["reason"].as_str().unwrap()
    );
    assert_eq!(card.scope.paths, vec!["src/main.rs".to_owned()]);
    assert!(card.allows("restore"));
    assert_eq!(card.display_state(1_000), InboxCardState::Actionable);
    assert!(card.display_state(1_000).is_actionable());
    assert!(!card.is_expired(1_000));
    assert!(card.is_expired(2_000));
    assert_eq!(card.display_state(2_000), InboxCardState::Expired);
    assert_eq!(card.fields.len(), 1);
    assert_eq!(card.fields[0].name, "comment");
    assert!(card.fields[0].required);
    assert_eq!(
        card.allowed_decisions,
        vec![
            "approve".to_owned(),
            "deny".to_owned(),
            "edit".to_owned(),
            "restore".to_owned(),
            "continue".to_owned(),
        ]
    );
    let protocol = card.as_protocol_card().unwrap();
    assert_eq!(protocol.action_id, "approval-15");
    assert_eq!(protocol.expected_revision, Some(4));
    assert_eq!(protocol.expires_at_unix_ms, Some(2_000));
    assert_eq!(protocol.payload_digest, card.payload_digest);
}

#[test]
fn expired_and_revoked_cards_are_never_actionable_and_never_produce_a_decision() {
    let mut inbox = WorkbenchInbox::default();
    inbox.replace(vec![card()]).unwrap();
    assert_eq!(
        inbox.state_of("approval-15", 1_000),
        Some(InboxCardState::Actionable)
    );
    assert_eq!(inbox.actionable(1_000).len(), 1);
    assert_eq!(
        inbox.state_of("approval-15", 2_000),
        Some(InboxCardState::Expired)
    );
    assert!(inbox.actionable(2_000).is_empty());
    // an expired card stays visible: the panel removes it from the actionable list only
    assert_eq!(inbox.card_count(), 1);
    assert_eq!(inbox.cards().count(), 1);
    let expired = inbox.get("approval-15").unwrap().clone();
    assert_eq!(
        expired.prepare_decision("approve", expired.payload.clone(), Some(4), 2_000),
        Err("approval_expired".to_owned())
    );
    inbox.revoke("approval-15").unwrap();
    assert_eq!(
        inbox.state_of("approval-15", 1_000),
        Some(InboxCardState::Revoked)
    );
    assert!(inbox.actionable(1_000).is_empty());
    assert_eq!(inbox.card_count(), 1);
    let revoked = inbox.get("approval-15").unwrap().clone();
    assert_eq!(
        revoked.prepare_decision("approve", revoked.payload.clone(), Some(4), 1_000),
        Err("approval_revoked".to_owned())
    );
    let mut hand_revoked = card();
    hand_revoked.revoke();
    assert_eq!(hand_revoked.display_state(1_000), InboxCardState::Revoked);
    assert_eq!(
        hand_revoked.prepare_decision("approve", hand_revoked.payload.clone(), Some(4), 1_000),
        Err("approval_revoked".to_owned())
    );
    for state in InboxCardState::ALL {
        assert!(!state.as_str().is_empty());
        assert_eq!(
            serde_json::to_value(state).unwrap().as_str(),
            Some(state.as_str())
        );
    }
}

#[test]
fn revision_mismatch_and_client_payload_mutation_fail_before_an_action_exists() {
    let card = card();
    let mut mismatched = card.clone();
    mismatched.payload_digest = DIGEST.to_owned();
    assert_eq!(
        mismatched.validate(),
        Err("inbox_payload_digest_mismatch".to_owned())
    );
    assert_eq!(
        card.prepare_decision("approve", card.payload.clone(), Some(3), 1_000),
        Err("approval_revision_mismatch".to_owned())
    );
    assert_eq!(
        card.prepare_decision(
            "approve",
            json!({"decision":"approve", "scope":"all/"}),
            Some(4),
            1_000
        ),
        Err("client_payload_mutation".to_owned())
    );
    assert_eq!(
        card.prepare_decision("delete", card.payload.clone(), Some(4), 1_000),
        Err("approval_decision_not_allowed".to_owned())
    );
    assert_eq!(
        card.prepare_decision("", card.payload.clone(), Some(4), 1_000),
        Err("inbox_decision_invalid".to_owned())
    );
    let decision = card
        .prepare_decision("deny", card.payload.clone(), Some(4), 1_000)
        .unwrap();
    assert_eq!(decision.decision, "deny");
    assert_eq!(decision.validate(), Ok(()));
}

#[test]
fn every_declared_decision_produces_its_stable_decision_intent() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let declared: Vec<&str> = fixture["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(
        declared,
        WorkbenchDecision::ALL
            .into_iter()
            .map(WorkbenchDecision::as_str)
            .collect::<Vec<_>>()
    );
    let card = card();
    for decision in WorkbenchDecision::ALL {
        let intent = card
            .prepare_decision(decision.as_str(), card.payload.clone(), Some(4), 1_000)
            .unwrap();
        assert_eq!(intent.decision, decision.as_str());
        assert_eq!(intent.action_id, card.action_id);
        assert_eq!(intent.command, card.command);
        assert_eq!(intent.target_id, card.target_id);
        assert_eq!(intent.expected_revision, Some(4));
        assert_eq!(intent.payload, card.payload);
        assert_eq!(intent.payload_digest, card.payload_digest);
        assert_eq!(intent.validate(), Ok(()));
        assert_eq!(WorkbenchDecision::parse(decision.as_str()), Some(decision));
        assert_eq!(
            serde_json::to_value(decision).unwrap().as_str(),
            Some(decision.as_str())
        );
    }
    assert_eq!(WorkbenchDecision::parse("delete"), None);
    assert_eq!(WorkbenchDecision::parse(""), None);
    assert_eq!(WorkbenchDecision::ALL.len(), 5);
}

#[test]
fn inbox_decisions_are_restricted_to_the_declared_vocabulary() {
    let mut unsupported = card();
    unsupported.allowed_decisions = vec!["approve".to_owned(), "delete".to_owned()];
    assert_eq!(
        unsupported.validate(),
        Err("inbox_allowed_decision_unsupported".to_owned())
    );
    assert_eq!(
        unsupported.prepare_decision("approve", unsupported.payload.clone(), Some(4), 1_000),
        Err("inbox_allowed_decision_unsupported".to_owned())
    );
    let mut empty = card();
    empty.allowed_decisions.clear();
    assert_eq!(
        empty.validate(),
        Err("inbox_allowed_decisions_missing".to_owned())
    );
    let payload = json!({"decision": "delete"});
    let forged = WorkbenchInboxDecision {
        action_id: "approval-15".to_owned(),
        command: "approval.decide".to_owned(),
        target_id: "run-15".to_owned(),
        decision: "delete".to_owned(),
        expected_revision: Some(4),
        payload_digest: json_digest(&payload),
        payload,
    };
    assert_eq!(
        forged.validate(),
        Err("inbox_decision_value_unsupported".to_owned())
    );
    let mut blank = card()
        .prepare_decision("approve", card().payload, Some(4), 1_000)
        .unwrap();
    blank.action_id = String::new();
    assert_eq!(
        blank.validate(),
        Err("inbox_decision_action_id_invalid".to_owned())
    );
}

#[test]
fn hand_built_decisions_must_still_match_their_payload_digest() {
    let card = card();
    let mut mutated = card
        .prepare_decision("approve", card.payload.clone(), Some(4), 1_000)
        .unwrap();
    mutated.payload = json!({"decision":"approve", "scope":"all/"});
    assert_eq!(
        mutated.validate(),
        Err("inbox_decision_payload_digest_mismatch".to_owned())
    );
    let mut malformed = card
        .prepare_decision("approve", card.payload.clone(), Some(4), 1_000)
        .unwrap();
    malformed.payload_digest = "not-a-digest".to_owned();
    assert_eq!(
        malformed.validate(),
        Err("inbox_decision_payload_digest_invalid".to_owned())
    );
    let mut zero_revision = card
        .prepare_decision("approve", card.payload.clone(), Some(4), 1_000)
        .unwrap();
    zero_revision.expected_revision = Some(0);
    assert_eq!(
        zero_revision.validate(),
        Err("inbox_decision_revision_invalid".to_owned())
    );
    let mut oversized = card
        .prepare_decision("approve", card.payload.clone(), Some(4), 1_000)
        .unwrap();
    oversized.payload = json!({"blob": "x".repeat(MAX_INBOX_PAYLOAD_BYTES)});
    oversized.payload_digest = json_digest(&oversized.payload);
    assert_eq!(
        oversized.validate(),
        Err("inbox_decision_payload_limit".to_owned())
    );
    let intact = card
        .prepare_decision("approve", card.payload.clone(), Some(4), 1_000)
        .unwrap();
    assert_eq!(intact.validate(), Ok(()));
}

#[test]
fn inbox_bounds_scope_fields_decisions_and_payload_size() {
    let mut oversized_scope = card();
    oversized_scope.scope.paths = vec!["src/main.rs".to_owned(); MAX_SCOPE_PATHS + 1];
    assert_eq!(
        oversized_scope.validate(),
        Err("inbox_scope_paths_limit".to_owned())
    );
    let mut url_path = card();
    url_path.scope.paths = vec!["https://example.invalid/diff".to_owned()];
    assert_eq!(
        url_path.validate(),
        Err("inbox_scope_path_url_forbidden".to_owned())
    );
    let mut bad_scope_digest = card();
    bad_scope_digest.scope.scope_digest = "sha256:short".to_owned();
    assert_eq!(
        bad_scope_digest.validate(),
        Err("inbox_scope_digest_invalid".to_owned())
    );
    let mut bad_path = card();
    bad_path.scope.paths = vec!["src/\0main.rs".to_owned()];
    assert_eq!(
        bad_path.validate(),
        Err("inbox_scope_path_invalid".to_owned())
    );
    let mut too_many_fields = card();
    too_many_fields.fields = (0..MAX_INBOX_FIELDS + 1)
        .map(|index| field(&format!("field-{index}")))
        .collect();
    assert_eq!(
        too_many_fields.validate(),
        Err("inbox_fields_limit".to_owned())
    );
    let mut bad_field = card();
    bad_field.fields = vec![InboxField {
        name: "comment".to_owned(),
        label: "Review\0comment".to_owned(),
        field_type: "text".to_owned(),
        required: true,
        allowed_values: Vec::new(),
    }];
    assert_eq!(
        bad_field.validate(),
        Err("inbox_field_label_invalid".to_owned())
    );
    let mut too_many_decisions = card();
    too_many_decisions.allowed_decisions = (0..MAX_INBOX_DECISIONS + 1)
        .map(|index| {
            WorkbenchDecision::ALL[index % WorkbenchDecision::ALL.len()]
                .as_str()
                .to_owned()
        })
        .collect();
    assert_eq!(
        too_many_decisions.validate(),
        Err("inbox_allowed_decisions_limit".to_owned())
    );
    let mut duplicate_decisions = card();
    duplicate_decisions.allowed_decisions = vec!["approve".to_owned(), "approve".to_owned()];
    assert_eq!(
        duplicate_decisions.validate(),
        Err("inbox_allowed_decision_duplicate".to_owned())
    );
    let mut blank_identity = card();
    blank_identity.command = String::new();
    assert_eq!(
        blank_identity.validate(),
        Err("inbox_command_invalid".to_owned())
    );
    blank_identity.command = "approval.decide".to_owned();
    blank_identity.target_id = String::new();
    assert_eq!(
        blank_identity.validate(),
        Err("inbox_target_id_invalid".to_owned())
    );
    blank_identity.target_id = "run-15".to_owned();
    blank_identity.reason = String::new();
    assert_eq!(
        blank_identity.validate(),
        Err("inbox_reason_invalid".to_owned())
    );
    blank_identity.reason = "The requested write needs operator review.".to_owned();
    blank_identity.payload_digest = "not-a-digest".to_owned();
    assert_eq!(
        blank_identity.validate(),
        Err("inbox_payload_digest_invalid".to_owned())
    );
    let mut zero_revision = card();
    zero_revision.expected_revision = Some(0);
    assert_eq!(
        zero_revision.validate(),
        Err("inbox_card_revision_invalid".to_owned())
    );
    let mut oversized_payload = card();
    oversized_payload.payload = json!({"blob": "x".repeat(MAX_INBOX_PAYLOAD_BYTES)});
    oversized_payload.payload_digest = json_digest(&oversized_payload.payload);
    assert_eq!(
        oversized_payload.validate(),
        Err("inbox_payload_limit".to_owned())
    );
    let mut wrong_schema = card();
    wrong_schema.schema = "kiana.workbench-review.v2".to_owned();
    assert_eq!(
        wrong_schema.validate(),
        Err("workbench_inbox_schema_invalid".to_owned())
    );
}

#[test]
fn inbox_deduplicates_cards_and_updates_are_never_a_silent_downgrade() {
    let card = card();
    let mut inbox = WorkbenchInbox::default();
    inbox.replace(vec![card.clone()]).unwrap();
    assert_eq!(inbox.card_count(), 1);
    assert_eq!(
        inbox.replace(vec![card.clone(), card.clone()]),
        Err("inbox_action_duplicate".to_owned())
    );
    assert_eq!(inbox.card_count(), 1);
    assert_eq!(
        inbox.replace(vec![card.clone(); MAX_INBOX_CARDS + 1]),
        Err("inbox_cards_limit".to_owned())
    );
    assert_eq!(inbox.card_count(), 1);

    // a refresher at the same or a newer expected revision replaces the retained card
    let mut refreshed = card.clone();
    refreshed.expected_revision = Some(5);
    refreshed.expires_at_unix_ms = Some(4_000);
    let previous = inbox.upsert(refreshed.clone()).unwrap().unwrap();
    assert_eq!(previous.expected_revision, Some(4));
    assert_eq!(inbox.card_count(), 1);
    assert_eq!(
        inbox.state_of("approval-15", 3_000),
        Some(InboxCardState::Actionable)
    );

    // a card whose expected object revision moves backwards is stale, not a silent overwrite
    assert_eq!(
        inbox.upsert(card.clone()),
        Err("inbox_card_revision_stale".to_owned())
    );
    let mut unpinned = refreshed.clone();
    unpinned.expected_revision = None;
    assert_eq!(
        inbox.upsert(unpinned),
        Err("inbox_card_revision_stale".to_owned())
    );
    assert_eq!(inbox.get("approval-15").unwrap().expected_revision, Some(5));

    // revocation is a fact: a later non-revoked payload cannot resurrect the card
    let mut revoked = refreshed.clone();
    revoked.revoke();
    inbox.upsert(revoked).unwrap();
    assert_eq!(
        inbox.state_of("approval-15", 3_000),
        Some(InboxCardState::Revoked)
    );
    assert_eq!(
        inbox.upsert(refreshed.clone()),
        Err("inbox_card_revocation_regression".to_owned())
    );
    assert_eq!(inbox.revoke("approval-15"), Ok(()));
    assert_eq!(inbox.revoke(""), Err("inbox_action_id_invalid".to_owned()));
    assert_eq!(
        inbox.revoke("approval-unknown"),
        Err("inbox_action_unknown".to_owned())
    );

    // an invalid card never enters the projection
    let mut invalid = refreshed.clone();
    invalid.reason = String::new();
    assert_eq!(
        inbox.upsert(invalid),
        Err("inbox_reason_invalid".to_owned())
    );
    assert_eq!(inbox.card_count(), 1);

    // removal is explicit and never resurrects a card
    assert!(inbox.remove("approval-15").is_some());
    assert_eq!(inbox.card_count(), 0);
    assert_eq!(inbox.state_of("approval-15", 1_000), None);
    assert!(inbox.actionable(1_000).is_empty());
}

#[test]
fn artifact_pages_reject_malformed_and_forged_byte_digests() {
    let reference = artifact(b"abc");
    let page = ArtifactPage {
        artifact_ref: reference.clone(),
        revision: 7,
        page_index: 0,
        page_count: 1,
        page_digest: reference.content_hash.clone(),
        content: b"abd".to_vec(),
    };
    for forged in [
        reference.content_hash.clone(),
        format!("sha256:{}", reference.content_hash),
    ] {
        let mut forged_page = page.clone();
        forged_page.page_digest = forged;
        assert_eq!(
            forged_page.validate(),
            Err("artifact_page_digest_mismatch".to_owned())
        );
    }
    for invalid in [
        String::new(),
        "0".repeat(63),
        "g".repeat(64),
        "sha256:".to_owned(),
        format!("sha256:{}", "0".repeat(65)),
        format!("sha512:{}", reference.content_hash),
    ] {
        let mut invalid_page = page.clone();
        invalid_page.page_digest = invalid;
        assert_eq!(
            invalid_page.validate(),
            Err("artifact_page_digest_invalid".to_owned())
        );
    }
}

#[test]
fn artifact_viewer_rejects_changed_reference_and_assembled_content() {
    let reference = artifact(b"abcdef");
    let mut viewer = ArtifactViewer::new(reference.clone(), 7).unwrap();
    let page = ArtifactPage {
        artifact_ref: reference.clone(),
        revision: 7,
        page_index: 0,
        page_count: 1,
        page_digest: reference.content_hash.clone(),
        content: b"abcdef".to_vec(),
    };
    let mut wrong_scope = page.clone();
    wrong_scope.artifact_ref.scope_digest = json_digest(&json!({"scope": "all/"}));
    let mut wrong_version = page.clone();
    wrong_version.artifact_ref.version += 1;
    let mut wrong_id = page.clone();
    wrong_id.artifact_ref.artifact_id = ArtifactId::new();
    let mut wrong_provenance = page;
    wrong_provenance.artifact_ref.provenance.recorded_by = "other-daemon".to_owned();
    for changed in [wrong_scope, wrong_version, wrong_id, wrong_provenance] {
        assert_eq!(
            viewer.accept_page(changed),
            Err("artifact_reference_mismatch".to_owned())
        );
    }
    assert_eq!(viewer.page_count(), 0);
    assert!(!viewer.is_complete());
    assert_eq!(viewer.display_state(), ArtifactViewState::Empty);
    assert!(viewer.content().is_none());

    viewer
        .accept_page(ArtifactPage {
            artifact_ref: reference.clone(),
            revision: 7,
            page_index: 0,
            page_count: 2,
            page_digest: page_digest(&reference, b"abc"),
            content: b"abc".to_vec(),
        })
        .unwrap();
    let mut last = ArtifactPage {
        artifact_ref: reference.clone(),
        revision: 7,
        page_index: 1,
        page_count: 2,
        page_digest: page_digest(&reference, b"dxf"),
        content: b"dxf".to_vec(),
    };
    assert_eq!(
        viewer.accept_page(last.clone()),
        Err("artifact_digest_mismatch".to_owned())
    );
    assert_eq!(viewer.page_count(), 1);
    assert!(!viewer.is_complete());
    assert_eq!(viewer.display_state(), ArtifactViewState::Partial);
    assert!(viewer.content().is_none());
    last.page_digest = page_digest(&reference, b"def");
    last.content = b"def".to_vec();
    viewer.accept_page(last).unwrap();
    assert!(viewer.is_complete());
    assert_eq!(viewer.display_state(), ArtifactViewState::Complete);
    assert_eq!(viewer.content().unwrap().as_slice(), b"abcdef");
}

#[test]
fn artifact_viewer_accepts_verified_bare_and_prefixed_sha256_hashes() {
    const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    for (prefixed_reference, prefixed_page) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let mut reference = artifact(b"abc");
        assert_eq!(reference.content_hash, SHA256_ABC);
        if prefixed_reference {
            reference.content_hash = format!("sha256:{SHA256_ABC}");
        }
        let page = ArtifactPage {
            artifact_ref: reference.clone(),
            revision: 7,
            page_index: 0,
            page_count: 1,
            page_digest: if prefixed_page {
                format!("sha256:{SHA256_ABC}")
            } else {
                SHA256_ABC.to_owned()
            },
            content: b"abc".to_vec(),
        };
        let mut viewer = ArtifactViewer::new(reference, 7).unwrap();
        viewer.accept_page(page.clone()).unwrap();
        viewer.accept_page(page).unwrap();
        assert_eq!(viewer.page_count(), 1);
        assert!(viewer.is_complete());
        assert_eq!(viewer.content().unwrap().as_slice(), b"abc");
    }
}

#[test]
fn artifact_pagination_is_partial_until_every_server_page_is_verified() {
    let full = b"abcdef";
    let reference = artifact(full);
    let mut viewer = ArtifactViewer::new(reference.clone(), 8).unwrap();
    assert_eq!(viewer.display_state(), ArtifactViewState::Empty);
    assert!(!viewer.display_state().is_complete());
    assert_eq!(viewer.retained_bytes(), 0);
    assert!(viewer.missing_pages().is_empty());
    assert_eq!(viewer.content(), None);
    let first = ArtifactPage {
        artifact_ref: reference.clone(),
        revision: 8,
        page_index: 0,
        page_count: 2,
        page_digest: page_digest(&reference, b"abc"),
        content: b"abc".to_vec(),
    };
    viewer.accept_page(first.clone()).unwrap();
    assert_eq!(viewer.display_state(), ArtifactViewState::Partial);
    assert_eq!(viewer.missing_pages(), vec![1]);
    assert_eq!(viewer.retained_bytes(), 3);
    assert_eq!(viewer.retained_pages().len(), 1);
    assert_eq!(viewer.content(), None);
    assert_eq!(viewer.content_bytes().unwrap().as_slice(), b"abc");

    // a different page for the same index is a conflict and changes nothing
    let conflicting = ArtifactPage {
        page_digest: page_digest(&reference, b"xyz"),
        content: b"xyz".to_vec(),
        ..first.clone()
    };
    assert_eq!(
        viewer.accept_page(conflicting),
        Err("artifact_page_conflict".to_owned())
    );
    assert_eq!(viewer.retained_bytes(), 3);
    assert_eq!(viewer.page_count(), 1);

    // a page set claiming another count is refused before any content is retained
    let other_count = ArtifactPage {
        page_index: 1,
        page_count: 3,
        page_digest: page_digest(&reference, b"def"),
        content: b"def".to_vec(),
        ..first.clone()
    };
    assert_eq!(
        viewer.accept_page(other_count),
        Err("artifact_page_count_mismatch".to_owned())
    );
    assert_eq!(viewer.retained_bytes(), 3);

    // the last page with the wrong bytes cannot complete the artifact
    let mut wrong_last = ArtifactPage {
        page_index: 1,
        page_digest: page_digest(&reference, b"xef"),
        content: b"xef".to_vec(),
        ..first.clone()
    };
    assert_eq!(
        viewer.accept_page(wrong_last.clone()),
        Err("artifact_digest_mismatch".to_owned())
    );
    assert_eq!(viewer.display_state(), ArtifactViewState::Partial);
    assert_eq!(viewer.missing_pages(), vec![1]);
    assert_eq!(viewer.page_count(), 1);

    // the verified last page completes the artifact and clears the missing list
    wrong_last.page_digest = page_digest(&reference, b"def");
    wrong_last.content = b"def".to_vec();
    viewer.accept_page(wrong_last.clone()).unwrap();
    assert_eq!(viewer.display_state(), ArtifactViewState::Complete);
    assert!(viewer.missing_pages().is_empty());
    assert_eq!(viewer.retained_bytes(), 6);
    assert_eq!(viewer.retained_pages().len(), 2);
    assert_eq!(viewer.content().unwrap().as_slice(), full);

    // replaying the same page is idempotent
    viewer.accept_page(wrong_last).unwrap();
    assert_eq!(viewer.page_count(), 2);
    assert_eq!(viewer.retained_bytes(), 6);
}

#[test]
fn artifact_pages_outside_the_declared_bounds_are_refused() {
    let reference = artifact(b"abc");
    let page = ArtifactPage {
        artifact_ref: reference.clone(),
        revision: 7,
        page_index: 0,
        page_count: 1,
        page_digest: SHA256_ABC.to_owned(),
        content: b"abc".to_vec(),
    };
    let mut outside = page.clone();
    outside.page_index = 1;
    assert_eq!(
        outside.validate(),
        Err("artifact_page_bounds_invalid".to_owned())
    );
    let mut zero_count = page.clone();
    zero_count.page_count = 0;
    assert_eq!(
        zero_count.validate(),
        Err("artifact_page_bounds_invalid".to_owned())
    );
    let mut too_many_pages = page.clone();
    too_many_pages.page_count = MAX_ARTIFACT_PAGES as u32 + 1;
    assert_eq!(
        too_many_pages.validate(),
        Err("artifact_page_bounds_invalid".to_owned())
    );
    let mut zero_revision = page.clone();
    zero_revision.revision = 0;
    assert_eq!(
        zero_revision.validate(),
        Err("artifact_page_bounds_invalid".to_owned())
    );
    let mut oversized = page.clone();
    oversized.content = vec![0u8; MAX_ARTIFACT_PAGE_BYTES + 1];
    oversized.page_digest = page_digest(&reference, &oversized.content);
    assert_eq!(
        oversized.validate(),
        Err("artifact_page_bounds_invalid".to_owned())
    );

    assert_eq!(
        ArtifactViewer::new(reference.clone(), 0),
        Err("artifact_expected_revision_invalid".to_owned())
    );
    let mut viewer = ArtifactViewer::new(reference.clone(), 9).unwrap();
    assert_eq!(
        viewer.accept_page(page.clone()),
        Err("artifact_revision_mismatch".to_owned())
    );
    assert_eq!(viewer.page_count(), 0);
    assert_eq!(viewer.retained_bytes(), 0);

    // the viewer never retains more bytes than the declared artifact budget
    let chunk = vec![7u8; MAX_ARTIFACT_PAGE_BYTES];
    let big_reference = artifact(&chunk.repeat(64));
    let mut big_viewer = ArtifactViewer::new(big_reference.clone(), 11).unwrap();
    for index in 0..64u32 {
        big_viewer
            .accept_page(ArtifactPage {
                artifact_ref: big_reference.clone(),
                revision: 11,
                page_index: index,
                page_count: 65,
                page_digest: page_digest(&big_reference, &chunk),
                content: chunk.clone(),
            })
            .unwrap();
    }
    assert_eq!(big_viewer.retained_bytes(), MAX_ARTIFACT_BYTES);
    assert_eq!(big_viewer.display_state(), ArtifactViewState::Partial);
    assert_eq!(
        big_viewer.accept_page(ArtifactPage {
            artifact_ref: big_reference.clone(),
            revision: 11,
            page_index: 64,
            page_count: 65,
            page_digest: page_digest(&big_reference, &chunk),
            content: chunk,
        }),
        Err("artifact_content_too_large".to_owned())
    );
    assert_eq!(big_viewer.retained_bytes(), MAX_ARTIFACT_BYTES);
    assert_eq!(big_viewer.page_count(), 64);
}

#[test]
fn receipt_keeps_files_cost_limitations_provenance_and_unknown_visible() {
    let receipt = unknown_receipt();
    receipt.validate().unwrap();
    assert_eq!(receipt.display_state(), ReceiptDisplayState::ResultUnknown);
    assert!(!receipt.is_green());
    let complete = receipt
        .recompute(
            ExecutionStatus::Completed,
            receipt.files.clone(),
            ReceiptCost {
                currency: "USD".to_owned(),
                amount_micros: Some(1200),
                usage_known: true,
                cost_known: true,
            },
        )
        .unwrap();
    assert_eq!(
        complete.display_state(),
        ReceiptDisplayState::CompletedWithLimitations
    );
    assert!(!complete.is_green());
    let mut fully_verified = complete;
    fully_verified.limitations.clear();
    assert!(fully_verified.is_green());
    assert_eq!(fully_verified.files.len(), 1);
    assert_eq!(fully_verified.cost.amount_micros, Some(1200));
    assert_eq!(
        fully_verified.provenance.source_event_ids,
        vec!["event-15".to_owned()]
    );

    let mut marker_dropped = unknown_receipt();
    marker_dropped.unknown = false;
    assert_eq!(
        marker_dropped.validate(),
        Err("result_unknown_marker_missing".to_owned())
    );
    let mut cost_mismatch = unknown_receipt();
    cost_mismatch.cost = ReceiptCost {
        currency: "USD".to_owned(),
        amount_micros: Some(1200),
        usage_known: false,
        cost_known: false,
    };
    assert_eq!(
        cost_mismatch.validate(),
        Err("receipt_cost_known_mismatch".to_owned())
    );
    let mut url_file = unknown_receipt();
    url_file.files[0].path = "https://example.invalid/diff".to_owned();
    assert_eq!(
        url_file.validate(),
        Err("receipt_file_url_forbidden".to_owned())
    );
    let mut blank_file = unknown_receipt();
    blank_file.files[0].path = String::new();
    assert_eq!(
        blank_file.validate(),
        Err("receipt_file_path_invalid".to_owned())
    );
    let mut zero_file_revision = unknown_receipt();
    zero_file_revision.files[0].revision = 0;
    assert_eq!(
        zero_file_revision.validate(),
        Err("receipt_file_revision_invalid".to_owned())
    );
    let mut blank_limitation = unknown_receipt();
    blank_limitation.limitations = vec![String::new()];
    assert_eq!(
        blank_limitation.validate(),
        Err("receipt_limitation_invalid".to_owned())
    );
    let mut too_many_files = unknown_receipt();
    too_many_files.files = vec![unknown_receipt().files[0].clone(); MAX_RECEIPT_FILES + 1];
    assert_eq!(
        too_many_files.validate(),
        Err("receipt_files_limit".to_owned())
    );
    let mut too_many_limitations = unknown_receipt();
    too_many_limitations.limitations =
        vec!["provider_receipt_pending".to_owned(); MAX_RECEIPT_LIMITATIONS + 1];
    assert_eq!(
        too_many_limitations.validate(),
        Err("receipt_limitations_limit".to_owned())
    );
    let mut bad_digest = unknown_receipt();
    bad_digest.provenance.receipt_digest = "receipt-15".to_owned();
    assert_eq!(
        bad_digest.validate(),
        Err("receipt_digest_invalid".to_owned())
    );
    let mut bad_schema = unknown_receipt();
    bad_schema.schema = "kiana.workbench-receipt.v2".to_owned();
    assert_eq!(
        bad_schema.validate(),
        Err("workbench_receipt_schema_invalid".to_owned())
    );
    let mut zero_cursor = unknown_receipt();
    zero_cursor.provenance.source_cursor = 0;
    assert_eq!(
        zero_cursor.validate(),
        Err("receipt_source_cursor_invalid".to_owned())
    );
    let mut no_events = unknown_receipt();
    no_events.provenance.source_event_ids.clear();
    assert_eq!(
        no_events.validate(),
        Err("receipt_source_events_missing".to_owned())
    );
    let mut duplicate_events = unknown_receipt();
    duplicate_events.provenance.source_event_ids =
        vec!["event-15".to_owned(), "event-15".to_owned()];
    assert_eq!(
        duplicate_events.validate(),
        Err("receipt_source_event_duplicate".to_owned())
    );
}

#[test]
fn receipt_never_paints_result_unknown_as_green_completed() {
    let receipt = unknown_receipt();
    assert_eq!(receipt.display_state(), ReceiptDisplayState::ResultUnknown);
    assert_eq!(receipt.display_state().as_str(), "result_unknown");
    assert!(!receipt.display_state().is_green());
    assert!(!receipt.is_green());
    let golden = receipt.golden().unwrap();
    assert_eq!(golden.display, "result_unknown");
    assert!(!golden.green);
    assert!(golden.unknown);

    let mut completed_but_unknown = unknown_receipt();
    completed_but_unknown.status = ExecutionStatus::Completed;
    assert_eq!(
        completed_but_unknown.display_state(),
        ReceiptDisplayState::ResultUnknown
    );
    assert!(!completed_but_unknown.is_green());
    assert!(!completed_but_unknown.golden().unwrap().green);

    let mut green = unknown_receipt();
    green.status = ExecutionStatus::Completed;
    green.unknown = false;
    green.limitations.clear();
    assert!(green.is_green());
    assert!(green.golden().unwrap().green);
    green
        .limitations
        .push("provider_receipt_pending".to_owned());
    assert!(!green.is_green());
    assert_eq!(
        green.display_state(),
        ReceiptDisplayState::CompletedWithLimitations
    );

    for state in ReceiptDisplayState::ALL {
        assert!(!state.as_str().is_empty());
        assert_eq!(
            serde_json::to_value(state).unwrap().as_str(),
            Some(state.as_str())
        );
    }
    assert_eq!(
        ReceiptDisplayState::ALL
            .iter()
            .filter(|state| state.is_green())
            .count(),
        1
    );
}

#[test]
fn receipt_replay_is_idempotent_and_never_regresses_a_terminal_state() {
    let mut receipt = unknown_receipt();

    // a lower cursor is stale
    let mut stale = unknown_receipt();
    stale.provenance.source_cursor = receipt.provenance.source_cursor - 1;
    assert_eq!(
        receipt.apply_server_receipt(stale),
        Err("receipt_replay_stale".to_owned())
    );
    // the same cursor must carry the same receipt digest, status and unknown marker
    let mut other_digest = unknown_receipt();
    other_digest.provenance.receipt_digest = json_digest(&json!({"receipt": "other"}));
    assert_eq!(
        receipt.apply_server_receipt(other_digest),
        Err("receipt_replay_conflict".to_owned())
    );
    let mut flipped = unknown_receipt();
    flipped.status = ExecutionStatus::Failed;
    flipped.unknown = false;
    assert_eq!(
        receipt.apply_server_receipt(flipped),
        Err("receipt_replay_conflict".to_owned())
    );
    // an identical replay is idempotent and changes nothing
    assert_eq!(
        receipt.apply_server_receipt(unknown_receipt()),
        Ok(ReceiptUpdate::Replayed)
    );
    assert_eq!(receipt.status, ExecutionStatus::ResultUnknown);
    assert!(receipt.unknown);

    // another run's receipt is refused outright
    let mut foreign = unknown_receipt();
    foreign.run_id = RunId::new();
    foreign.provenance.source_cursor = 16;
    assert_eq!(
        receipt.apply_server_receipt(foreign),
        Err("receipt_run_mismatch".to_owned())
    );

    // a strictly newer server receipt may resolve the unknown state
    let mut resolved = unknown_receipt();
    resolved.provenance.source_cursor = 16;
    resolved.status = ExecutionStatus::Completed;
    resolved.unknown = false;
    resolved.cost = ReceiptCost {
        currency: "USD".to_owned(),
        amount_micros: Some(1200),
        usage_known: true,
        cost_known: true,
    };
    assert_eq!(
        receipt.apply_server_receipt(resolved),
        Ok(ReceiptUpdate::Applied)
    );
    assert!(receipt.is_terminal());
    assert!(!receipt.is_green());
    assert_eq!(
        receipt.display_state(),
        ReceiptDisplayState::CompletedWithLimitations
    );

    // a terminal receipt cannot be moved back to a running state
    let mut regressed = unknown_receipt();
    regressed.provenance.source_cursor = 17;
    regressed.status = ExecutionStatus::Running;
    regressed.unknown = false;
    assert_eq!(
        receipt.apply_server_receipt(regressed),
        Err("receipt_terminal_regression".to_owned())
    );
    assert_eq!(
        receipt.display_state(),
        ReceiptDisplayState::CompletedWithLimitations
    );

    // a non-terminal receipt can still be advanced
    let mut live = unknown_receipt();
    live.status = ExecutionStatus::Running;
    live.unknown = false;
    let mut advanced = live.clone();
    let mut cancelled = live;
    cancelled.provenance.source_cursor = 16;
    cancelled.status = ExecutionStatus::Cancelled;
    assert_eq!(
        advanced.apply_server_receipt(cancelled),
        Ok(ReceiptUpdate::Applied)
    );
    assert_eq!(advanced.display_state(), ReceiptDisplayState::Cancelled);
    assert!(advanced.is_terminal());

    // an invalid candidate never lands
    let mut invalid = unknown_receipt();
    invalid.provenance.source_cursor = 16;
    invalid.limitations = vec![String::new()];
    assert_eq!(
        receipt.apply_server_receipt(invalid),
        Err("receipt_limitation_invalid".to_owned())
    );
    for update in ReceiptUpdate::ALL {
        assert!(!update.as_str().is_empty());
        assert_eq!(
            serde_json::to_value(update).unwrap().as_str(),
            Some(update.as_str())
        );
    }
}

#[test]
fn receipt_recompute_refuses_inconsistent_server_fragments() {
    let receipt = unknown_receipt();
    let mut broken_file = receipt.files[0].clone();
    broken_file.revision = 0;
    assert_eq!(
        receipt
            .recompute(
                ExecutionStatus::Completed,
                vec![broken_file],
                receipt.cost.clone()
            )
            .err(),
        Some("receipt_file_revision_invalid".to_owned())
    );
    let rebuilt = receipt
        .recompute(
            ExecutionStatus::Cancelled,
            receipt.files.clone(),
            receipt.cost.clone(),
        )
        .unwrap();
    assert_eq!(rebuilt.run_id, receipt.run_id);
    assert_eq!(
        rebuilt.provenance.receipt_digest,
        receipt.provenance.receipt_digest
    );
    assert_eq!(rebuilt.limitations, receipt.limitations);
    assert_eq!(rebuilt.display_state(), ReceiptDisplayState::Cancelled);
    assert!(!rebuilt.is_green());
    let again = receipt
        .recompute(
            ExecutionStatus::ResultUnknown,
            Vec::new(),
            receipt.cost.clone(),
        )
        .unwrap();
    assert!(again.unknown);
    assert!(!again.is_green());
    assert!(again.files.is_empty());
}

#[test]
fn receipt_golden_is_deterministic_and_keeps_unknown_and_limitations_visible() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let receipt = unknown_receipt();
    let golden = receipt.golden().unwrap();
    assert_eq!(golden.schema, WORKBENCH_RECEIPT_GOLDEN_SCHEMA);
    assert_eq!(golden.run_id, RUN_UUID);
    assert_eq!(
        serde_json::to_value(golden.clone()).unwrap(),
        fixture["receipt"]["golden"]
    );
    assert_eq!(golden.file_count, 1);
    assert_eq!(golden.provenance.source_cursor, 15);
    assert_eq!(
        golden.limitations,
        vec!["provider_receipt_pending".to_owned()]
    );
    assert_eq!(
        serde_json::to_string(&receipt.golden().unwrap()).unwrap(),
        serde_json::to_string(&golden).unwrap()
    );

    // files are sorted and digests normalized, so caller order cannot change the golden
    let mut files = receipt.files.clone();
    files.push(ReceiptFile {
        path: "src/lib.rs".to_owned(),
        status: "added".to_owned(),
        revision: 2,
        artifact_ref: Some(artifact(b"abc")),
    });
    let forward = receipt
        .recompute(
            ExecutionStatus::ResultUnknown,
            files.clone(),
            receipt.cost.clone(),
        )
        .unwrap()
        .golden()
        .unwrap();
    files.reverse();
    let reversed = receipt
        .recompute(ExecutionStatus::ResultUnknown, files, receipt.cost.clone())
        .unwrap()
        .golden()
        .unwrap();
    assert_eq!(forward, reversed);
    assert_eq!(forward.files[0].path, "src/lib.rs");
    assert_eq!(forward.files[1].path, "src/main.rs");
    assert_eq!(
        forward.files[0].artifact_digest.as_deref(),
        Some(SHA256_ABC)
    );
    assert!(!forward.green);
    assert!(forward.unknown);
    assert_eq!(
        forward.limitations,
        vec!["provider_receipt_pending".to_owned()]
    );

    let encoded = serde_json::to_string(&golden).unwrap();
    let decoded: ReceiptGolden = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, golden);
    let mut forged = serde_json::to_value(&golden).unwrap();
    forged["client_override"] = json!("approve");
    assert!(serde_json::from_value::<ReceiptGolden>(forged).is_err());
}

#[test]
fn protocol_card_conversion_does_not_add_client_fields() {
    let card = card();
    let protocol = card.as_protocol_card().unwrap();
    assert_eq!(protocol.allowed_decisions.len(), 5);
    assert_eq!(protocol.expected_revision, Some(4));
    let restored = WorkbenchInboxCard::from_protocol(
        HumanActionCard {
            action_id: protocol.action_id,
            command: protocol.command,
            target_id: protocol.target_id,
            expected_revision: protocol.expected_revision,
            expires_at_unix_ms: protocol.expires_at_unix_ms,
            allowed_decisions: protocol.allowed_decisions,
            payload_digest: protocol.payload_digest,
        },
        card.reason,
        card.scope,
        card.fields,
        card.payload,
    )
    .unwrap();
    assert_eq!(restored.action_id, "approval-15");
    assert_eq!(restored.validate(), Ok(()));
}

#[test]
fn review_refusal_catalogue_covers_every_presenter_visible_refusal() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let declared: Vec<&str> = fixture["denied"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(declared, ReviewRefusal::codes());
    assert_eq!(declared.len(), REVIEW_REFUSALS.len());
    assert!(!REVIEW_REFUSALS.is_empty());
    let mut stages = Vec::new();
    for code in &declared {
        let refusal = ReviewRefusal::find(code).expect("every declared refusal must be catalogued");
        assert!(
            !refusal.recovery.trim().is_empty(),
            "the refusal {code} has no recovery note"
        );
        assert!(refusal.recovery.len() <= 120);
        stages.push(refusal.stage.as_str());
    }
    for stage in ReviewRefusalStage::ALL {
        assert!(
            stages.contains(&stage.as_str()),
            "no presenter-visible refusal is catalogued for stage {}",
            stage.as_str()
        );
        assert_eq!(
            serde_json::to_value(stage).unwrap().as_str(),
            Some(stage.as_str())
        );
    }
    assert_eq!(ReviewRefusal::find("does_not_exist"), None);

    // every catalogue entry is asserted by name in a behaviour fixture in this file
    let fixtures = include_str!("ui15_workbench_review.rs");
    for code in &declared {
        assert!(
            fixtures.contains(&format!("\"{code}\"")),
            "no behaviour fixture asserts the catalogued refusal {code}"
        );
    }
    let pinned: Vec<&str> = fixture["recovery"]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(
        pinned,
        ReviewRefusalStage::ALL.map(|stage| stage.as_str()).to_vec()
    );
    assert_eq!(fixture["recovery"]["catalogue"], "REVIEW_REFUSALS");
}

#[test]
fn review_contracts_round_trip_through_typed_input() {
    let card = card();
    let decoded_card: WorkbenchInboxCard =
        serde_json::from_str(&serde_json::to_string(&card).unwrap()).unwrap();
    assert_eq!(decoded_card, card);
    let decision = card
        .prepare_decision("approve", card.payload.clone(), Some(4), 1_000)
        .unwrap();
    let decoded_decision: WorkbenchInboxDecision =
        serde_json::from_str(&serde_json::to_string(&decision).unwrap()).unwrap();
    assert_eq!(decoded_decision, decision);
    let page = ArtifactPage {
        artifact_ref: artifact(b"abc"),
        revision: 7,
        page_index: 0,
        page_count: 1,
        page_digest: SHA256_ABC.to_owned(),
        content: b"abc".to_vec(),
    };
    page.validate().unwrap();
    let decoded_page: ArtifactPage =
        serde_json::from_str(&serde_json::to_string(&page).unwrap()).unwrap();
    assert_eq!(decoded_page, page);
    let receipt = unknown_receipt();
    let decoded_receipt: WorkbenchReceipt =
        serde_json::from_str(&serde_json::to_string(&receipt).unwrap()).unwrap();
    assert_eq!(decoded_receipt, receipt);

    // every strict decoder refuses client-invented fields
    let mut forged_card = serde_json::to_value(&card).unwrap();
    forged_card["client_scope_override"] = json!("all/");
    assert!(serde_json::from_value::<WorkbenchInboxCard>(forged_card).is_err());
    let mut forged_decision = serde_json::to_value(&decision).unwrap();
    forged_decision["approve_now"] = json!(true);
    assert!(serde_json::from_value::<WorkbenchInboxDecision>(forged_decision).is_err());
    let mut forged_receipt = serde_json::to_value(&receipt).unwrap();
    forged_receipt["green"] = json!(true);
    assert!(serde_json::from_value::<WorkbenchReceipt>(forged_receipt).is_err());
    assert!(serde_json::from_value::<InboxCardState>(json!("actionable_now")).is_err());
    assert!(serde_json::from_value::<ArtifactViewState>(json!("almost_complete")).is_err());
}

#[test]
fn declared_scenarios_are_exercised_by_their_named_fixtures() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let declared: Vec<&str> = fixture["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    let fixtures = include_str!("ui15_workbench_review.rs");
    assert!(declared.len() >= 20);
    for name in &declared {
        assert_eq!(
            fixtures.matches(&format!("fn {name}(")).count(),
            1,
            "UI-15 fixture is not defined exactly once in kiana-entrypoints/tests/ui15_workbench_review.rs: {name}"
        );
    }
    let scenarios = fixture["scenarios"].as_array().unwrap();
    assert!(scenarios.len() >= 10);
    for scenario in scenarios {
        assert!(!scenario["id"].as_str().unwrap().is_empty());
        let name = scenario["fixture"].as_str().unwrap();
        assert!(
            declared.contains(&name),
            "scenario {} points at an undeclared fixture {name}",
            scenario["id"]
        );
        for refusal in scenario["refusals"].as_array().unwrap() {
            let refusal = refusal.as_str().unwrap();
            assert!(
                ReviewRefusal::find(refusal).is_some(),
                "scenario {} references an uncatalogued refusal {refusal}",
                scenario["id"]
            );
        }
    }
    assert!(fixture["limitations"].as_array().unwrap().len() >= 4);
    assert_eq!(fixture["diff"]["client_side_diff"], "forbidden");
    assert_eq!(fixture["diff"]["source"], "server_artifact_ref");
}
