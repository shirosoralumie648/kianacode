//! CP-21 source guard for rebuildable projections, owner-scoped read paths and receipts.
//!
//! The behavioral fixtures run in GitHub Actions.  This guard keeps the cross-crate contract
//! visible in the source tree without treating a local cache, UI stream or receipt as authority.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CP-21 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn cp_query_changes_neither_ledger_nor_execution() {
    let projection = include_str!("../src/projection.rs");
    let checkpoint = include_str!("../src/projection_checkpoint.rs");
    let history = include_str!("../src/history.rs");
    let receipts = include_str!("../src/receipts.rs");
    let recovery = include_str!("../src/recovery.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    require(
        projection,
        &[
            "project_run_state",
            "RunProjectionError",
            "TerminalConflict",
            "read_all_events",
            "invocation_projection_event_ids",
            "run_not_found",
        ],
        "run projection",
    );
    require(
        checkpoint,
        &[
            "ProjectionDriver",
            "ProjectionDriverStatus::Paused",
            "ProjectionDriverStatus::RebuildRequired",
            "projection_checkpoint_cursor_gap",
            "projection_checkpoint_cursor_not_advanced",
            "source_event_ids",
        ],
        "projection checkpoint",
    );
    require(
        history,
        &[
            "model_visible_history",
            "read_all_events",
            "event_is_in_scope",
            "未知事件 kind",
            "no replay authority",
        ],
        "history query",
    );
    require(
        receipts,
        &[
            "events_for_persisted_run",
            "receipt_owner_mismatch",
            "typed_run_receipt",
            "typed_execution_receipts",
            "aggregate_receipt_facts",
            "files_changed_from_events",
            "result_unknown",
        ],
        "receipt query",
    );
    require(
        recovery,
        &[
            "list_pending_approvals",
            "requested_run",
            "read_decision",
            "context_for_pending",
            "ApprovalView",
        ],
        "pending query",
    );
    require(
        ports,
        &[
            "ProjectionStorePort",
            "apply_events",
            "read_projection",
            "checkpoint",
            "projection_store_unsupported",
        ],
        "projection port",
    );

    // A read model cannot become a second command path or issue execution authority.
    for source in [projection, checkpoint, history, receipts] {
        for forbidden in [
            "issue_permit(",
            "execute_authorized_request(",
            "broker.execute",
            "append_event(",
        ] {
            assert!(
                !source.contains(forbidden),
                "CP-21 read path contains execution/ledger mutation marker: {forbidden}"
            );
        }
    }
}

#[test]
fn cp_fresh_projection_matches_live_receipt() {
    let projection = include_str!("../src/projection.rs");
    let invocation = include_str!("../src/invocation_projection.rs");
    let attempts = include_str!("../src/capability_attempt_projection.rs");
    let receipts = include_str!("../src/receipts.rs");
    let domain_receipts = include_str!("../../kiana-domain/src/receipt_contracts.rs");
    let checkpoint = include_str!("../src/projection_checkpoint.rs");

    require(
        invocation,
        &[
            "project_invocations",
            "invocation_event_run_id_conflict",
            "invocation_terminal_conflict",
            "CapabilityExecutionState::Unknown",
            "event_ids",
        ],
        "invocation projection",
    );
    require(
        attempts,
        &[
            "project_capability_attempts",
            "source_cursor",
            "source_event_ids",
            "effect_known",
            "stop_confirmed",
            "fenced",
            "terminal_digest",
        ],
        "attempt projection",
    );
    require(
        receipts,
        &[
            "cache_invocation_projection",
            "typed_run_receipt",
            "typed_execution_receipts",
            "aggregate_receipt_facts",
            "invocation_projection_error",
            "run_receipt",
            "execution_receipts",
        ],
        "live receipt",
    );
    require(
        domain_receipts,
        &[
            "owner_actor_id",
            "source_cursor",
            "source_event_ids",
            "result_digest",
            "redaction_profile",
            "receipt_digest",
            "effect_known",
            "fenced",
        ],
        "typed receipt contract",
    );
    require(
        checkpoint,
        &[
            "from_checkpoint",
            "state_digest",
            "source_cursor",
            "ProjectionCheckpoint",
        ],
        "fresh rebuild",
    );
    require(
        projection,
        &["read_all_events", "cache_invocation_projection"],
        "live source",
    );
}

#[test]
fn cp_cross_project_query_cannot_disclose_pending_or_receipt() {
    let receipts = include_str!("../src/receipts.rs");
    let recovery = include_str!("../src/recovery.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");

    require(
        receipts,
        &[
            "receipt_owner_mismatch",
            "run_owner_mismatch",
            "canonical_project_root",
            "run_data_revoked",
            "retained_event_ids",
        ],
        "receipt owner filter",
    );
    require(
        recovery,
        &[
            "list_pending_approvals",
            "resolve_run_id",
            "self.approvals.list_pending(context)",
            "context_for_pending",
            "redact_event_value",
        ],
        "pending owner filter",
    );
    require(
        daemon,
        &[
            "ui_snapshot",
            "ui_events",
            "persisted_events",
            "self.principal.actor_id",
            "project_root",
            "run.authorized",
        ],
        "daemon owner filter",
    );
    require(
        protocol,
        &[
            "ListApprovals",
            "ApprovalListRequest",
            "Receipt",
            "ReceiptRequest",
        ],
        "read-only wire query",
    );
    for forbidden in [
        "initialize_assignment",
        "refresh_authority",
        "consume_approval",
        "issue_permit",
    ] {
        assert!(
            !receipts.contains(forbidden),
            "receipt query must not {forbidden}"
        );
    }
}
