//! CAP-23 source guard for fair admission, conservative footprints and cancellation fencing.

#[path = "support/source_guard.rs"]
mod source_guard;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-23 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn admission_is_bounded_fair_and_never_an_execution_loop() {
    let scheduler = include_str!("../src/capability_scheduler.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let cell = include_str!("../src/cell_registry.rs");
    let sessions = include_str!("../src/sessions.rs");
    let daemon = include_str!("../../kiana-daemon/src/execution_control.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");

    require(
        scheduler,
        &[
            "CapabilityAdmissionScheduler",
            "MAX_ACTIVE_ADMISSIONS",
            "MAX_PENDING_ADMISSIONS",
            "FootprintMode::Read",
            "FootprintMode::Write",
            "FootprintMode::Unknown",
            "canonical_paths",
            "earlier_conflict",
            "active_conflict",
            "quarantine_conflict",
            "cancellation.changed()",
            "cancelled:before_dispatch",
            "capability_scheduler_queue_full",
            "AdmissionOutcome::Unknown",
            "state.quarantined",
            "never invokes a handler",
        ],
        "admission scheduler",
    );
    require(
        capabilities,
        &[
            "admission_scheduler",
            ".acquire(request",
            "begin_cell_capability_from_request",
            "finish_cell_capability",
            "AdmissionOutcome::Unknown",
        ],
        "ControlPlane dispatch gate",
    );
    require(
        cell,
        &[
            "path_lock_conflict",
            "budget_ledger",
            ".reserve(plan.budget_reservation)",
            "begin_capability",
            "finish_capability",
            "cell_capability_concurrency_exceeded",
            "resources_released",
        ],
        "CellRegistry authority",
    );
    require(
        sessions,
        &[
            "acquire_durable_path_locks",
            "LOCK_NB",
            "path_lock_conflict",
        ],
        "cross-process lock",
    );
    require(
        daemon,
        &[
            "acquire_workspace_resources",
            "capacity",
            "ProcessSupervisor",
        ],
        "daemon resource boundary",
    );
    require(
        runner,
        &[
            "pending_tools.drain(..)",
            "cancelled:queued",
            "ToolCancelled",
        ],
        "queued cancellation",
    );
    require(
        baseline,
        &[
            "overlapping_write_scopes_never_execute_concurrently",
            "parallel_calls_cannot_overspend_parent_budget",
            "cancelled_queued_call_never_spawns",
        ],
        "CAP-23 acceptance card",
    );
    let scheduler_code = source_guard::production_source(scheduler);
    assert!(!scheduler_code.contains("CapabilityBroker"));
    assert!(!scheduler_code.contains("tokio::spawn"));
    assert!(!scheduler_code.contains("model loop"));
}

#[test]
fn unknown_and_cancelled_admissions_have_distinct_boundaries() {
    let scheduler = include_str!("../src/capability_scheduler.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let cancellation = include_str!("../../kiana-domain/src/cancellation.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");

    assert!(scheduler.contains("quarantined.push(footprint)"));
    assert!(scheduler.contains("AdmissionOutcome::Unknown"));
    assert!(capabilities.contains("result_unknown:cancel_stop_unconfirmed"));
    assert!(capabilities.contains("not_executed"));
    assert!(cancellation.contains("RunCancellationState::ResultUnknown"));
    assert!(lifecycle.contains("run.result_unknown"));
}

#[test]
fn cap23_behavioural_fixtures_are_wired_and_deny_first() {
    let scheduler = include_str!("../src/capability_scheduler.rs");
    let cell = include_str!("../src/cell_registry.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let lane = include_str!("../../.github/workflows/cap23-concurrency-scheduler.yml");
    let ci = include_str!("../../.github/workflows/ci.yml");
    let baseline = include_str!("../../docs/roadmap/cap23-concurrency-scheduler-baseline.md");

    let fixtures = [
        (
            "capability_scheduler.rs",
            scheduler,
            "overlapping_write_scopes_never_execute_concurrently",
        ),
        (
            "capability_scheduler.rs",
            scheduler,
            "cancelled_queued_call_never_spawns",
        ),
        (
            "capability_scheduler.rs",
            scheduler,
            "pre_cancelled_requests_never_enter_the_queue_or_take_a_slot",
        ),
        (
            "capability_scheduler.rs",
            scheduler,
            "read_only_requests_share_and_disjoint_writes_run_concurrently",
        ),
        (
            "capability_scheduler.rs",
            scheduler,
            "unknown_result_quarantines_its_scope_from_successors",
        ),
        (
            "capability_scheduler.rs",
            scheduler,
            "released_or_rejected_admissions_clean_up_their_slot_and_queue_is_bounded",
        ),
        (
            "cell_registry.rs",
            cell,
            "parallel_calls_cannot_overspend_parent_budget",
        ),
        (
            "cell_registry.rs",
            cell,
            "cell_concurrency_and_duplicate_fences_share_one_lease",
        ),
    ];
    for (module, source, fixture) in fixtures {
        assert!(
            source.contains(&format!("async fn {fixture}(")),
            "CAP-23 fixture {module}::{fixture} is missing from the production module"
        );
        assert!(
            lane.contains(fixture),
            "CAP-23 fixture {fixture} is not wired into the focused lane"
        );
        assert!(
            baseline.contains(fixture),
            "CAP-23 fixture {fixture} is not recorded in the baseline"
        );
    }
    // Behaviour fixtures are evidence, not placeholders: they must not be ignored or removed.
    assert!(!scheduler.contains("#[ignore]"));
    assert!(!cell.contains("#[ignore]"));

    // The focused lane and the automatic unified lane must both run the in-module fixtures.
    for lane_source in [lane, ci] {
        assert!(
            lane_source.contains("cargo test -p kiana-core --lib capability_scheduler::tests::")
        );
        assert!(lane_source.contains("cargo test -p kiana-core --lib cell_registry::tests::"));
    }
    assert!(lane.contains("--exact"));

    // Deny-first ordering inside the focused lane: the deny fixtures run before the
    // concurrency/regression fixtures that depend on them.
    let deny_write = lane
        .find("overlapping_write_scopes_never_execute_concurrently")
        .expect("deny-first write fixture in the lane");
    let read_success = lane
        .find("read_only_requests_share_and_disjoint_writes_run_concurrently")
        .expect("read/disjoint write fixture in the lane");
    assert!(
        deny_write < read_success,
        "deny-first fixtures must run before the read/disjoint-write regression fixture"
    );
    let deny_budget = lane
        .find("parallel_calls_cannot_overspend_parent_budget")
        .expect("deny-first budget fixture in the lane");
    let budget_regression = lane
        .find("cell_concurrency_and_duplicate_fences_share_one_lease")
        .expect("budget regression fixture in the lane");
    assert!(
        deny_budget < budget_regression,
        "deny-first fixtures must run before the budget/concurrency regression fixture"
    );

    // Write footprints must come from the server-derived scope, not from command text.
    let footprint = source_guard::item_source(scheduler, "fn from_request(");
    assert!(footprint.contains("write_roots"));
    assert!(
        !footprint.contains("arguments"),
        "the admission footprint must not parse request arguments"
    );

    // The slot is acquired inside the dispatch path only, after the approval decision is
    // already final, and before the Cell budget lease is taken.
    let dispatch = source_guard::item_source(capabilities, "async fn dispatch_capability_action(");
    let acquire_at = dispatch
        .find("admission_scheduler")
        .expect("dispatch must acquire an admission lease");
    let cell_at = dispatch
        .find("begin_cell_capability_from_request")
        .expect("dispatch must take the Cell lease after admission");
    assert!(
        acquire_at < cell_at,
        "the admission gate must run before the Cell budget lease"
    );
    let approval = source_guard::item_source(capabilities, "async fn broker_harness_capability(");
    assert!(
        !approval.contains("admission_scheduler"),
        "approval staging must not hold an execution slot"
    );

    // Denied Cell calls must stay inside the budget/concurrency fences.
    assert!(cell.contains(".consume(1, 0, effect_count)"));
    assert!(cell.contains("cell_capability_invocation_duplicate"));
    assert!(cell.contains("cell_capability_concurrency_exceeded"));
}
