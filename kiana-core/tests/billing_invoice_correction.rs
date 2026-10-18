//! BQ-22 correction-loop fixtures and source guard (deny-first).
//!
//! The domain crate proves the invoice facts and the local parser; this target proves the missing
//! caller: an invoice discrepancy handed to the existing BQ-14 approval path through
//! [`ProviderInvoiceAdmission::admit_invoice_correction`].  An unapproved draft is refused before
//! BQ-14 is asked, an approved command is admitted exactly once, and a forged ledger-entry digest
//! can never be reconciled.  All fixtures are in-memory: no EventStore append, no approval
//! consumption, no provider call, no network, no filesystem.
//!
//! The guard test pins the core module's boundary: it classifies domain refusals, it does not own
//! a second correction ledger, dispatch loop or model-visible tool surface.

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{ControlPlane, ProviderInvoiceAdmission, ProviderInvoiceAdmissionError};
use kiana_domain::{
    compare_provider_invoice, ApprovalId, AttemptId, CostCorrectionApproval, CostCorrectionCommand,
    CostCorrectionId, CostLedgerEntry, CostLedgerEntryKind, EventId, InvoiceComparisonState,
    InvoiceLedgerExpectation, LedgerEntryId, Money, ProviderInvoiceCorrectionHandoff,
    ProviderInvoiceImport, ProviderReceiptRef, RateCardId, RequestId, RunId,
    PROVIDER_INVOICE_SCHEMA,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::Arc;

const CORE_SOURCE: &str = include_str!("../src/billing_invoice.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/billing_invoice.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq22-invoice.yml");
const BASELINE: &str = include_str!("../../docs/roadmap/bq22-invoice-baseline.md");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

const D1: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D3: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const NOW: u64 = 200_000_000;

fn money(micros: i128) -> Money {
    Money::new("USD", micros).expect("money")
}

fn receipt(value: &str) -> ProviderReceiptRef {
    ProviderReceiptRef::new(value.to_owned()).expect("receipt ref")
}

/// A committed measured ledger fact with period, model and provider receipt.
fn ledger_entry(cost_micros: i128) -> CostLedgerEntry {
    CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        Some(money(cost_micros)),
        "USD",
        10,
        EventId::new(),
        1,
        1,
        D1,
    )
    .expect("entry")
    .with_measured_cost(money(cost_micros), receipt("receipt:provider-1"))
    .expect("measured")
    .with_billing_period(1_000, 2_000)
    .expect("period")
    .with_model("model-a")
    .expect("model")
}

fn invoice(amount_micros: i128) -> ProviderInvoiceImport {
    let mut import = ProviderInvoiceImport {
        schema: PROVIDER_INVOICE_SCHEMA.to_owned(),
        provider_account_ref: "account:provider-1".to_owned(),
        invoice_id: "invoice:2026-09".to_owned(),
        period_start_unix_ms: 1_000,
        period_end_unix_ms: 2_000,
        model: Some("model-a".to_owned()),
        usage_digest: Some(D1.to_owned()),
        amount: money(amount_micros),
        receipt_ref: receipt("receipt:provider-1"),
        receipt_digest: D3.to_owned(),
        authentication_ref: "verified:provider-1".to_owned(),
        authenticated: true,
        source_cursor: 12,
        import_key: "import:provider-1:2026-09".to_owned(),
        import_digest: String::new(),
    };
    import.import_digest = kiana_domain::json_digest(&serde_json::json!({
        "schema": import.schema,
        "provider_account_ref": import.provider_account_ref,
        "invoice_id": import.invoice_id,
        "period_start_unix_ms": import.period_start_unix_ms,
        "period_end_unix_ms": import.period_end_unix_ms,
        "model": import.model,
        "usage_digest": import.usage_digest,
        "amount": import.amount,
        "receipt_ref": import.receipt_ref,
        "receipt_digest": import.receipt_digest,
        "authentication_ref": import.authentication_ref,
        "authenticated": import.authenticated,
        "source_cursor": import.source_cursor,
        "import_key": import.import_key,
    }));
    import
}

/// Invoice at 120 against a committed 100 fact: an explicit amount discrepancy.
fn discrepancy(
    entry: &CostLedgerEntry,
) -> (
    ProviderInvoiceImport,
    InvoiceLedgerExpectation,
    ProviderInvoiceCorrectionHandoff,
) {
    let correction_id = CostCorrectionId::new();
    let correction_ref = correction_id.to_string();
    let import = invoice(120);
    let expectation = InvoiceLedgerExpectation::try_from(entry).expect("expectation");
    let comparison = compare_provider_invoice(&import, &expectation, Some(correction_ref.as_str()))
        .expect("comparison");
    assert_eq!(comparison.state, InvoiceComparisonState::CorrectionRequired);
    assert_eq!(
        comparison.discrepancy_codes,
        vec!["amount_mismatch".to_owned()]
    );
    let handoff = ProviderInvoiceCorrectionHandoff::from_import_comparison(&import, &comparison)
        .expect("handoff");
    assert_eq!(handoff.correction_ref, correction_ref);
    (import, expectation, handoff)
}

fn draft(
    handoff: &ProviderInvoiceCorrectionHandoff,
    entry: &CostLedgerEntry,
) -> CostCorrectionCommand {
    CostCorrectionCommand::draft(
        RequestId::new(),
        CostCorrectionId::parse_str(&handoff.correction_ref).expect("correction id round-trip"),
        entry,
        None,
        Some(handoff.delta_measured.clone()),
        "invoice amount_mismatch: provider receipt reconciliation",
        handoff.evidence_refs.clone(),
        "requester",
        "bq22-correction",
        NOW,
    )
    .expect("draft")
}

fn approve(draft: &CostCorrectionCommand, entry: &CostLedgerEntry) -> CostCorrectionCommand {
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "approver",
        NOW + 1,
    )
    .expect("approval");
    draft.clone().with_approval(approval).expect("approved")
}

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

struct UnavailableApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnavailableApprovalStore {
    async fn stage(
        &self,
        _context: &kiana_domain::RequestContext,
        _request: kiana_domain::CapabilityRequest,
        _reason: &str,
    ) -> Result<kiana_domain::ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn consume(
        &self,
        _context: &kiana_domain::RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<kiana_domain::PendingApproval, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }
}

fn plane() -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        Arc::new(MemoryEventLog::new()),
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnavailableApprovalStore),
        Arc::new(UnavailableRunner),
    )
}

#[test]
fn unapproved_invoice_draft_is_refused_and_approved_correction_is_admitted() {
    let entry = ledger_entry(100);
    let (_import, _expectation, handoff) = discrepancy(&entry);
    let draft = draft(&handoff, &entry);
    assert!(draft.approval.is_none());
    assert!(handoff.validate_draft_command(&draft).is_ok());

    // Deny-first: the draft can never enter the correction path without an approval bound to it.
    assert_eq!(
        ProviderInvoiceAdmission::admit_invoice_correction(&handoff, &entry, &draft),
        Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
            "cost_correction_approval_required".to_owned()
        ))
    );

    // Success column: the same command carrying an exact approval is admitted and yields a
    // correction fact whose binding is the committed target, not the invoice text.
    let approved = approve(&draft, &entry);
    let correction =
        ProviderInvoiceAdmission::admit_invoice_correction(&handoff, &entry, &approved)
            .expect("admitted correction");
    assert_eq!(correction.target_entry_digest, entry.entry_digest);
    assert_eq!(correction.delta_measured, Some(money(20)));
    assert_eq!(correction.requested_by, "requester");
    assert_eq!(correction.approver_id, "approver");
    assert_eq!(correction.correction_id.to_string(), handoff.correction_ref);
    assert!(correction.validate_against(&entry).is_ok());

    // The ControlPlane facade applies the same single rule (no second correction ledger).
    let through_plane = plane()
        .admit_provider_invoice_correction(&handoff, &entry, &approved)
        .expect("facade admits the same approved command");
    assert_eq!(through_plane, correction);
    assert_eq!(
        plane().admit_provider_invoice_correction(&handoff, &entry, &draft),
        Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
            "cost_correction_approval_required".to_owned()
        ))
    );
}

#[test]
fn forged_ledger_digest_and_drifted_approvals_are_refused_before_admission() {
    let entry = ledger_entry(100);
    let (_import, _expectation, handoff) = discrepancy(&entry);
    let draft = draft(&handoff, &entry);

    // A handoff built from a forged expectation (digest of some other entry) cannot be admitted
    // against the real committed entry: the target must be the authoritative fact.
    let mut forged_expectation = InvoiceLedgerExpectation::try_from(&entry).expect("expectation");
    forged_expectation.ledger_entry_digest = D3.to_owned();
    let import = invoice(120);
    let forged_comparison = compare_provider_invoice(
        &import,
        &forged_expectation,
        Some(handoff.correction_ref.as_str()),
    )
    .expect("forged comparison");
    let forged_handoff =
        ProviderInvoiceCorrectionHandoff::from_import_comparison(&import, &forged_comparison)
            .expect("forged handoff");
    let forged_draft = CostCorrectionCommand::draft(
        draft.command_id,
        draft.correction_id,
        &entry,
        None,
        Some(money(20)),
        "invoice amount_mismatch",
        forged_handoff.evidence_refs.clone(),
        "requester",
        "bq22-forged",
        NOW,
    )
    .expect("forged draft");
    let forged_approved = approve(&forged_draft, &entry);
    assert_eq!(
        ProviderInvoiceAdmission::admit_invoice_correction(
            &forged_handoff,
            &entry,
            &forged_approved
        ),
        Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
            "provider_invoice_handoff_target_digest_mismatch".to_owned()
        ))
    );

    // An approved command naming a different correction reference is refused: the handoff binds
    // one correction id, and approval does not widen it.
    let mut drifted = draft.clone();
    drifted.correction_id = CostCorrectionId::new();
    drifted.command_digest = drifted.digest();
    drifted.approval = Some(
        CostCorrectionApproval::new(
            ApprovalId::new(),
            drifted.command_id,
            drifted.command_digest.clone(),
            entry.entry_digest.clone(),
            "approver",
            NOW + 1,
        )
        .expect("approval"),
    );
    assert_eq!(
        ProviderInvoiceAdmission::admit_invoice_correction(&handoff, &entry, &drifted),
        Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
            "provider_invoice_handoff_command_reference_mismatch".to_owned()
        ))
    );

    // A command whose reason/evidence no longer carries the handoff facts is refused.
    let mut missing_reason = approve(&draft, &entry);
    missing_reason.reason = "invoice reconciliation".to_owned();
    assert_eq!(
        ProviderInvoiceAdmission::admit_invoice_correction(&handoff, &entry, &missing_reason),
        Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
            "provider_invoice_handoff_command_reason_missing".to_owned()
        ))
    );
}

#[test]
fn correction_loop_owns_no_ledger_dispatch_or_network_authority() {
    for marker in [
        "admit_invoice_correction",
        "CostCorrectionAdmission::authorize",
        "provider_invoice_handoff_target_digest_mismatch",
        "cost_correction_approval_required",
        "provider_invoice_correction_invalid",
        "admit_provider_invoice_correction",
    ] {
        assert!(
            CORE_SOURCE.contains(marker),
            "BQ-22 core correction loop marker missing: {marker}"
        );
    }
    for forbidden in [
        "fn append",
        "fn consume",
        "fn update",
        "fn delete",
        "EventStore::append",
        "CapabilityBrokerPort",
        "std::process::Command",
        "std::net",
        "std::fs",
        "reqwest",
        "tokio::spawn",
        "unsafe",
        "auto_success",
        "invoice_paid",
        "ToolSchema",
    ] {
        assert!(
            !CORE_SOURCE.contains(forbidden),
            "BQ-22 core correction loop widened authority: {forbidden}"
        );
    }
    // The domain fixtures stay deny-first: the forged-ledger fixture and its stable codes exist.
    for code in [
        "cost_ledger_entry_digest_mismatch",
        "provider_invoice_receipt_mismatch",
        "provider_invoice_ledger_entry_period_missing",
        "provider_invoice_ledger_entry_model_missing",
        "provider_invoice_document_amount_negative",
        "provider_invoice_document_schema_invalid",
        "provider_invoice_document_receipt_digest_invalid",
        "provider_invoice_document_import_key_invalid",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(code),
            "BQ-22 domain fixture refusal code missing: {code}"
        );
    }
}

#[test]
fn bq22_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test billing_invoice --locked -- --test-threads=1",
        "cargo test -p kiana-core --test billing_invoice_correction --locked -- --test-threads=1",
        "cargo test -p kiana-core --test billing_invoice_guard --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test billing_invoice_import --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test billing_invoice --no-run --locked",
        "cargo test -p kiana-core --test billing_invoice_correction --no-run --locked",
        "cargo test -p kiana-daemon --test billing_invoice_import --no-run --locked",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-22 manual lane missing: {command}"
        );
    }
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-22 manual lane must stay dispatch-only: {forbidden}"
        );
    }
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:"));

    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("shard registry json");
    let domain_targets = shards["kiana-domain-s1/4"]["targets"]
        .as_array()
        .expect("domain shard targets");
    assert!(domain_targets
        .iter()
        .any(|target| target.as_str() == Some("billing_invoice")));
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    for target in ["billing_invoice_guard", "billing_invoice_correction"] {
        assert!(
            core_targets
                .iter()
                .any(|entry| entry.as_str() == Some(target)),
            "BQ-22 core target missing from the auto shard: {target}"
        );
    }

    // The baseline records the same fixture names, so the card cannot claim an unrun path.
    for name in [
        "unapproved_invoice_draft_is_refused_and_approved_correction_is_admitted",
        "forged_ledger_digest_and_drifted_approvals_are_refused_before_admission",
        "correction_loop_owns_no_ledger_dispatch_or_network_authority",
    ] {
        assert!(
            BASELINE.contains(name),
            "BQ-22 baseline must pin the core fixture: {name}"
        );
    }
}
