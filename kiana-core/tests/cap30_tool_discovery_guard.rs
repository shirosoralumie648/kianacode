//! CAP-30 source guard: discovery admission is a decision, and a decision grants nothing.
//!
//! Every marker below was verified to appear literally in the referenced file before this guard
//! was written. The forbidden-string half is the part that matters most: `tool_discovery` is a
//! pure reducer, and the day it reaches for a filesystem, a broker or a port it stops being one.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-30 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn tool_discovery_admission_is_reduced_not_executed() {
    let discovery = include_str!("../../kiana-domain/src/tool_discovery.rs");
    require(
        discovery,
        &[
            "TOOL_DISCOVERY_SCHEMA",
            "TOOL_DISCOVERY_PLAN_SCHEMA",
            "TOOL_DISCOVERY_REPORT_SCHEMA",
            "TOOL_DISCOVERY_VERSION",
            "MAX_TOOL_DISCOVERY_CANDIDATES",
            "ToolDiscoveryOrigin",
            "ToolDiscoveryTrust",
            "ToolDiscoveryHealth",
            "ToolDiscoveryCandidate",
            "ToolDiscoveryPlan",
            "ToolDiscoveryReport",
            "ToolDiscoveryStatus",
            "ToolDiscoveryDenial",
            "BuiltinBindingConflict",
            "DuplicateToolName",
            "SourceUntrusted",
            "CatalogVersionStale",
            "SourceUnhealthy",
            "ProjectMismatch",
            "RoleNotBound",
            "EffectInsufficient",
            "ReadOnlyWriteDenied",
            "is_write_capability",
            "precedence",
            "shadows_builtin_descriptor",
            "check_manifest_effect",
            "tool_discovery_report_binding_invalid",
            "tool_discovery_manifest_read_only_write_scope",
            "does_not_grant_execution",
        ],
        "discovery admission",
    );
    // A reducer with a fixed decision order, plus a re-deriving validator, so a report cannot be
    // presented as a different pass's answer.
    require(
        discovery,
        &[
            "pub fn evaluate(",
            "pub fn validate_against(",
            "violations.sort_by_key(|denial| denial.precedence())",
        ],
        "fixed decision order",
    );
    // It is a decision, not an execution path. Nothing here may load, verify, spawn or dispatch.
    for forbidden in [
        "std::fs",
        "std::process",
        "tokio::",
        "EventStorePort",
        "CapabilityBroker",
        "reqwest",
        "std::net",
    ] {
        assert!(
            !discovery.contains(forbidden),
            "CAP-30 discovery crossed the effect boundary: {forbidden}"
        );
    }
}

#[test]
fn cap30_card_cases_are_fixtures_and_are_still_partial() {
    let fixtures = include_str!("../../kiana-domain/tests/cap30_tool_discovery.rs");
    require(
        fixtures,
        &[
            "fn tool_search_never_returns_invisible_or_untrusted_descriptors()",
            "fn extension_cannot_self_grant_or_replace_builtin_binding()",
            "fn read_only_extension_write_is_denied_at_executor()",
            "fn decision_order_is_fixed_and_a_tampered_report_is_rejected()",
        ],
        "rejected-first fixtures",
    );
    let baseline = include_str!("../../docs/roadmap/cap30-tool-extension-baseline.md");
    require(
        baseline,
        &[
            "tool_search_never_returns_invisible_or_untrusted_descriptors",
            "extension_cannot_self_grant_or_replace_builtin_binding",
            "read_only_extension_write_is_denied_at_executor",
            "partial",
            "BM25",
        ],
        "CAP-30 baseline",
    );
    // The baseline must not quietly claim the ranking or the live-extension work this slice
    // did not do.
    assert!(baseline.contains("does not prove"));
    assert!(baseline.contains("not established"));
}
