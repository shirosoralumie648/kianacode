//! BQ-23 daemon ingress guard: one command/query path, no second execution loop.
//!
//! `include_str!` pins the markers that make the daemon serve the query through the existing
//! `DaemonHost` path, and forbids a model client, a subprocess, a background task or an EventStore
//! append on that path. It is a text check; the behavioural proof lives in
//! `bq23_billing_query_surfaces.rs`.

#[test]
fn billing_query_ingress_is_identity_checked_and_not_a_second_execution_path() {
    let daemon = include_str!("../src/lib.rs");
    for marker in [
        "RequestBody::BillingQuery",
        "billing_query_invalid",
        "billing_query_unauthenticated",
        "request_may_execute",
        "billing_query_service::serve_billing_query(",
        "billing_query_service::serve_billing_query_command(",
        "billing_query_service::BILLING_QUERY_COMMAND",
        "budget.query.v1",
    ] {
        assert!(
            daemon.contains(marker),
            "BQ-23 daemon marker missing: {marker}"
        );
    }
    // The query is not an effect: it stays outside the assignment/authority synchronization path.
    let read_only_arm = daemon
        .split("| \"workspace.checkpoint.preview\"")
        .nth(1)
        .expect("read-only command allowlist");
    assert!(
        read_only_arm[..200].contains("budget.query.v1"),
        "BQ-23 command must stay on the read-only allowlist"
    );
    assert!(
        daemon.contains("if command.name == billing_query_service::BILLING_QUERY_COMMAND {"),
        "BQ-23 command intercept missing"
    );
    // The projection-not-wired refusal is gone: the host now serves the query.
    assert!(
        !daemon.contains("billing_query_projection_not_wired"),
        "BQ-23 daemon still refuses to serve the query"
    );
    // The query arm itself must stay a direct await: no model client, subprocess, background
    // task or EventStore append. `lib.rs` spawns elsewhere for unrelated flush work, so the
    // check is scoped to the query dispatch arm rather than the whole file.
    let arm = daemon
        .split("RequestBody::BillingQuery(query) => {")
        .nth(1)
        .expect("billing query dispatch arm")
        .split("RequestBody::Spawn(spawn) =>")
        .next()
        .expect("billing query arm end");
    assert!(
        arm.contains("return billing_query_service::serve_billing_query("),
        "BQ-23 query arm must return the read-only service directly"
    );
    for forbidden in [
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        ".append(",
    ] {
        assert!(
            !arm.contains(forbidden),
            "BQ-23 daemon query arm widened authority: {forbidden}"
        );
    }
}

#[test]
fn billing_query_service_is_a_projection_reader() {
    let service = include_str!("../src/billing_query_service.rs");
    for marker in [
        "BILLING_QUERY_COMMAND",
        "BILLING_QUERY_SURFACE_SCHEMA",
        "BILLING_QUERY_DEFAULT_LIMIT",
        "BILLING_QUERY_SERVICE_READ_ONLY",
        "project_billing_ledger",
        "query_billing_projection",
        "BillingQueryAdmission::admit",
        "billing_query_projection_source_cursor_missing",
        "billing_query_boundary_mismatch",
        "billing_query_identity_override_rejected",
    ] {
        assert!(
            service.contains(marker),
            "BQ-23 service marker missing: {marker}"
        );
    }
    for forbidden in [
        ".append(",
        "append_idempotent",
        "reserve_budget",
        "release_reservation",
        "consume_approval",
        "ModelClient",
        "tokio::spawn",
        "std::process::Command",
        "apply_patch",
    ] {
        assert!(
            !service.contains(forbidden),
            "BQ-23 service gained an effect or tool path: {forbidden}"
        );
    }
}
