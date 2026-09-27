//! PD-31 source guard: the conformance suite lives in ports, the capability matrix states the
//! proof boundary, and neither one performs an effect.
//!
//! Every marker below is asserted with `include_str!` + `contains`, so each one was grepped out of
//! the real source before this test was written. The forbidden list is checked against the same
//! files: a suite that opened a file, forked a process or drove a database engine would be
//! claiming runtime evidence from a source contract.

#[test]
fn pd31_the_suite_and_the_matrix_live_in_kiana_ports() {
    let ports = include_str!("../../kiana-ports/src/adapter_conformance.rs");
    for marker in [
        "pub struct AdapterConformance",
        "pub struct AdapterConformanceReport",
        "pub struct AdapterCapabilityMatrix",
        "pub enum AdapterKind",
        "pub enum ProofCeiling",
        "pub enum ConformanceCheck",
        "pub enum ConformanceResult",
        "pub enum UnsupportedProbe",
        "pub async fn run_event_store_conformance",
        "pub fn conformance_batch",
        "ADAPTER_CONFORMANCE_SCHEMA",
        "ADAPTER_DECLARATION_SCHEMA",
        "ADAPTER_CONFORMANCE_REPORT_SCHEMA",
        "ADAPTER_CAPABILITY_MATRIX_SCHEMA",
    ] {
        assert!(ports.contains(marker), "PD-31 marker missing: {marker}");
    }
    // The suite must be reachable from the crate root, or no adapter can run it.
    let lib = include_str!("../../kiana-ports/src/lib.rs");
    assert!(lib.contains("mod adapter_conformance;"));
    assert!(lib.contains("run_event_store_conformance"));
}

#[test]
fn pd31_the_rejected_first_column_is_named_in_the_source() {
    let ports = include_str!("../../kiana-ports/src/adapter_conformance.rs");
    for marker in [
        // adapter claims unsupported yet is called
        "adapter_unsupported_capability_succeeded",
        "adapter_refusal_code_undeclared_",
        "adapter_declaration_probe_code_not_declared",
        "adapter_declaration_probe_capability_supported",
        // memory mislabelled durable
        "adapter_declaration_memory_cannot_be_durable",
        "adapter_capability_matrix_memory_marked_durable",
        "adapter_capability_matrix_memory_row_missing",
        // ordering / error-code drift
        "adapter_cursor_page_order_invalid",
        "adapter_stream_order_invalid",
        "adapter_read_all_empty_success",
        "adapter_declaration_capabilities_mismatch",
        "adapter_atomic_flag_disagrees_with_capabilities",
        "adapter_durable_flag_disagrees_with_capabilities",
        "adapter_replay_returned_",
        "adapter_stale_commit_returned_",
        "adapter_conflicted_command_has_receipt",
        "adapter_check_not_run",
        // the durable/local_behavior/physical boundary
        "adapter_declaration_proof_ceiling_not_certifiable",
        "adapter_declaration_reserved_kind_not_implemented",
        "adapter_conformance_report_check_set_incomplete",
        "adapter_conformance_report_check_order_invalid",
        "adapter_capability_matrix_adapter_duplicate",
    ] {
        assert!(ports.contains(marker), "PD-31 marker missing: {marker}");
    }
}

#[test]
fn pd31_the_suite_cannot_certify_durable_or_physical() {
    let ports = include_str!("../../kiana-ports/src/adapter_conformance.rs");
    // The suite observes declarations and logical outcomes only. The top of the ladder is a claim
    // this source contract cannot make, so it must be refused rather than merely discouraged.
    assert!(ports.contains(
        "pub fn certifiable_by_source_suite(&self) -> bool {\n        *self <= Self::LocalBehavior\n    }"
    ));
    assert!(ports.contains("if !self.proof_ceiling.certifiable_by_source_suite() {"));
    // The ladder is ordered so a row may not skip a rung.
    assert!(ports.contains(
        "pub enum ProofCeiling {\n    Source,\n    LocalBehavior,\n    Durable,\n    Physical,\n}"
    ));
    // A reserved kind is a roadmap slot, not a shipped adapter.
    assert!(ports.contains("pub fn is_reserved(&self) -> bool {\n        matches!(self, Self::DurableFile | Self::Sqlite)\n    }"));
}

#[test]
fn pd31_the_suite_is_declaration_first_and_probe_last() {
    let ports = include_str!("../../kiana-ports/src/adapter_conformance.rs");
    // Declaration honesty is decided before any behaviour is observed, so a lying declaration is
    // reported as such instead of being discovered halfway through.
    let declaration_at = ports
        .find("ConformanceCheck::DeclarationMatchesAdapter,\n        if observed == declaration.capabilities")
        .expect("the declaration check is decided first");
    let first_behaviour_at = ports
        .find("let fresh = conformance_batch(")
        .expect("behaviour checks run after the declaration checks");
    assert!(
        declaration_at < first_behaviour_at,
        "PD-31 must decide the declaration before observing behaviour"
    );
    // The refusal probe runs last: probing a lying declaration would be meaningless.
    let probe_at = ports
        .find("let probed = probe().await;")
        .expect("the refusal probe runs");
    assert!(
        probe_at > first_behaviour_at,
        "PD-31 must probe refusal after the behavioural checks"
    );
}

#[test]
fn pd31_performs_no_io_and_crosses_no_effect_boundary() {
    let ports = include_str!("../../kiana-ports/src/adapter_conformance.rs");
    // A source conformance suite must not open, write, fork or connect. If one of these appears,
    // the slice has grown an effect it does not document.
    for forbidden in [
        "std::fs",
        "std::process",
        "std::net",
        "tokio::fs",
        "rusqlite",
        "sqlx",
        "File::",
        "OpenOptions",
        "CapabilityBroker",
        "execute_capability",
    ] {
        assert!(
            !ports.contains(forbidden),
            "PD-31 conformance suite crossed the effect boundary: {forbidden}"
        );
    }
}

#[test]
fn pd31_the_two_shipped_adapters_are_the_only_conforming_rows() {
    let ports = include_str!("../../kiana-ports/src/adapter_conformance.rs");
    let fixtures = include_str!("pd31_adapter_conformance.rs");
    // Memory and JSONL are the adapters that exist. A durable-file or SQLite adapter does not ship
    // in this checkout, so no fixture may claim to have run one against a real store.
    assert!(fixtures.contains("MemoryEventLog::new()"));
    assert!(fixtures.contains("JsonlEventLog::open("));
    assert!(!fixtures.contains("rusqlite"));
    assert!(!fixtures.contains("std::process"));
    // The matrix is built from declarations, never from a file or a database handle.
    assert!(ports.contains("pub struct AdapterCapabilityMatrix"));
    assert!(!ports.contains("PathBuf"));
}
