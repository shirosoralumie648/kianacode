use kiana_entrypoints::workbench_review::{
    ArtifactPage, ArtifactViewer, InboxField, InboxScope, ReceiptCost, ReceiptDisplayState,
    ReceiptFile, ReceiptProvenance, WorkbenchInbox, WorkbenchInboxCard, WorkbenchReceipt,
    MAX_ARTIFACT_PAGE_BYTES, WORKBENCH_RECEIPT_SCHEMA, WORKBENCH_REVIEW_SCHEMA,
};
use kiana_protocol::{
    json_digest, ArtifactId, ArtifactProvenance, ArtifactVersion, ExecutionStatus, HumanActionCard,
    RunId,
};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const FIXTURE: &str = include_str!("fixtures/ui15-workbench-review.json");

fn scope() -> InboxScope {
    InboxScope {
        scope_digest: DIGEST.to_owned(),
        summary: "workspace files under src/".to_owned(),
        paths: vec!["src/main.rs".to_owned()],
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
        fields: vec![InboxField {
            name: "comment".to_owned(),
            label: "Review comment".to_owned(),
            field_type: "text".to_owned(),
            required: true,
            allowed_values: Vec::new(),
        }],
        allowed_decisions: vec![
            "approve".to_owned(),
            "deny".to_owned(),
            "edit".to_owned(),
            "restore".to_owned(),
            "continue".to_owned(),
        ],
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

#[test]
fn fixture_and_inbox_display_contract_keep_server_fields() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fixture["schema"], WORKBENCH_REVIEW_SCHEMA);
    for marker in fixture["deny_first"].as_array().unwrap() {
        assert!(!marker.as_str().unwrap().is_empty());
    }
    let card = card();
    card.validate().unwrap();
    assert_eq!(card.reason, "The requested write needs operator review.");
    assert_eq!(card.scope.paths, vec!["src/main.rs"]);
    assert!(card.allows("restore"));
    assert!(card.is_expired(2_000));
    let protocol = card.as_protocol_card().unwrap();
    assert_eq!(protocol.action_id, "approval-15");
}

#[test]
fn deny_expiry_revision_and_client_payload_mutation_before_action() {
    let card = card();
    let mut mismatched = card.clone();
    mismatched.payload_digest = DIGEST.to_owned();
    assert_eq!(
        mismatched.validate(),
        Err("inbox_payload_digest_mismatch".to_owned())
    );
    assert_eq!(
        card.prepare_decision("approve", card.payload.clone(), Some(4), 2_000),
        Err("approval_expired".to_owned())
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
    let decision = card
        .prepare_decision("deny", card.payload.clone(), Some(4), 1_000)
        .unwrap();
    assert_eq!(decision.decision, "deny");

    let mut revoked = card;
    revoked.revoke();
    assert_eq!(
        revoked.prepare_decision("approve", revoked.payload.clone(), Some(4), 1_000),
        Err("approval_revoked".to_owned())
    );
}

#[test]
fn inbox_deduplicates_cards_and_artifact_viewer_fences_ref_revision_digest() {
    let card = card();
    let mut inbox = WorkbenchInbox::default();
    inbox.replace(vec![card.clone()]).unwrap();
    assert_eq!(inbox.cards().count(), 1);
    assert_eq!(
        inbox.replace(vec![card.clone(), card]),
        Err("inbox_action_duplicate".to_owned())
    );

    let content = b"@@ -1 +1 @@\n-old\n+new\n";
    let reference = artifact(content);
    let mut viewer = ArtifactViewer::new(reference.clone(), 7).unwrap();
    let page_digest = reference.content_hash.clone();
    viewer
        .accept_page(ArtifactPage {
            artifact_ref: reference.clone(),
            revision: 7,
            page_index: 0,
            page_count: 1,
            page_digest,
            content: content.to_vec(),
        })
        .unwrap();
    assert!(viewer.is_complete());
    assert_eq!(viewer.content().unwrap().as_slice(), content);

    let mut bad_revision = ArtifactPage {
        artifact_ref: reference.clone(),
        revision: 6,
        page_index: 0,
        page_count: 1,
        page_digest: reference.content_hash.clone(),
        content: content.to_vec(),
    };
    assert_eq!(
        viewer.accept_page(bad_revision.clone()),
        Err("artifact_revision_mismatch".to_owned())
    );
    bad_revision.revision = 7;
    bad_revision.page_digest = DIGEST.to_owned();
    assert_eq!(
        viewer.accept_page(bad_revision),
        Err("artifact_page_digest_mismatch".to_owned())
    );
    assert!(MAX_ARTIFACT_PAGE_BYTES > content.len());

    let full = b"abcdef";
    let paged_reference = artifact(full);
    let mut paged = ArtifactViewer::new(paged_reference.clone(), 8).unwrap();
    paged
        .accept_page(ArtifactPage {
            artifact_ref: paged_reference.clone(),
            revision: 8,
            page_index: 0,
            page_count: 2,
            page_digest: page_digest(&paged_reference, b"abc"),
            content: b"abc".to_vec(),
        })
        .unwrap();
    assert!(!paged.is_complete());
    assert!(paged.content().is_none());
    paged
        .accept_page(ArtifactPage {
            artifact_ref: paged_reference.clone(),
            revision: 8,
            page_index: 1,
            page_count: 2,
            page_digest: page_digest(&paged_reference, b"def"),
            content: b"def".to_vec(),
        })
        .unwrap();
    assert!(paged.is_complete());
    assert_eq!(paged.content().unwrap().as_slice(), full);
}

#[test]
fn receipt_keeps_files_cost_limitations_provenance_and_unknown_visible() {
    let receipt = WorkbenchReceipt {
        schema: WORKBENCH_RECEIPT_SCHEMA.to_owned(),
        run_id: RunId::new(),
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
    };
    receipt.validate().unwrap();
    assert_eq!(receipt.display_state(), ReceiptDisplayState::ResultUnknown);
    assert!(!receipt.is_green());
    let complete = receipt.recompute(
        ExecutionStatus::Completed,
        receipt.files.clone(),
        ReceiptCost {
            currency: "USD".to_owned(),
            amount_micros: Some(1200),
            usage_known: true,
            cost_known: true,
        },
    );
    assert_eq!(
        complete.display_state(),
        ReceiptDisplayState::CompletedWithLimitations
    );
    assert!(!complete.is_green());
    let mut fully_verified = complete;
    fully_verified.limitations.clear();
    assert!(fully_verified.is_green());
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
}
