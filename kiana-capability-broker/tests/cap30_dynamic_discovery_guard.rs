//! CAP-30 dynamic discovery source guard.
//!
//! Two jobs, both about the *shape* of the slice rather than its decisions:
//!
//! 1. the admission contract is actually reachable from the crate root, and the refusal vocabulary
//!    the baseline claims is present in source;
//! 2. the slice stayed a read-only contract — no process, no network, no plugin load, no event, no
//!    second execution loop, and no edit to the frozen five-tool model surface.
//!
//! The second job is the one that can rot silently. A decision function is easy to keep pure by
//! accident; it is easy to stop being pure by one `std::process::Command` in a later refactor, and
//! nothing else in the workspace would notice.

use std::collections::BTreeSet;

const DISCOVERY: &str = include_str!("../src/discovery.rs");
const BROKER: &str = include_str!("../src/lib.rs");
const RUNNER_TOOLS: &str = include_str!("../../kiana-runner/src/tools.rs");
const POLICY_GRANT_SCOPE: &str = include_str!("../../kiana-policy/src/grant_scope.rs");

/// Tokens that would mean the slice grew an effect. Each is matched against `discovery.rs` only,
/// so the same words appearing elsewhere in the workspace stay legal.
const FORBIDDEN_EFFECT_TOKENS: &[&str] = &[
    "std::process::Command",
    "process::Command",
    "tokio::process",
    "std::fs::",
    "tokio::fs",
    "File::open",
    "reqwest::",
    "TcpStream",
    "std::net::",
    "ExtensionManifest::from_json",
    "serde_json::from_str",
    "serde_json::from_value",
    "EventStore",
    "append_event",
    "handle_command",
    "CapabilityBrokerPort",
    "ControlPlane",
    "spawn_blocking",
    "async fn",
    ".await",
    "unsafe",
    "impl Drop",
    "static mut",
];

/// Tokens that would mean the slice invented a way to reach an effect without the ControlPlane.
const FORBIDDEN_CONTROL_FLOW_TOKENS: &[&str] = &[
    "register_static",
    "register_extension_static",
    "register_extension_component_static",
    "set_extension_admission",
    "handlers",
    "CapabilityBroker::new",
];

/// The module source with every comment line removed, so a prose mention of a forbidden concept
/// (for example a doc comment saying "this is not a ControlPlane call") cannot satisfy or trip a
/// token check. Token checks run against code.
fn code_only(source: &str) -> String {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_discovery_contract_is_registered_and_reachable_from_the_crate_root() {
    assert!(
        BROKER.contains("mod discovery;"),
        "discovery module is not declared in the broker crate root"
    );
    assert!(
        BROKER.contains("pub use discovery::*;"),
        "discovery contract is not re-exported, so no caller can reach it"
    );
    // Exactly two lines may mention the module: the declaration and the re-export. Anything more
    // means the slice has been wired into the broker's own execution path, which is the second
    // execution loop this card forbids. Discovery is a decision a caller makes *before* it builds
    // a request; the broker still routes only requests ControlPlane already authorized.
    let mentions = BROKER
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("discovery"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        mentions,
        BTreeSet::from([
            "mod discovery;".to_owned(),
            "pub use discovery::*;".to_owned()
        ]),
        "discovery must be declared and re-exported, and referenced nowhere else in the crate root"
    );
}

#[test]
fn the_refusal_vocabulary_is_present_in_source() {
    for marker in [
        // request level
        "discovery_request_invalid",
        "discovery_surface_invalid",
        "discovery_authorization_stale",
        "discovery_authorization_revoked",
        "discovery_authorization_expired",
        "discovery_search_failed",
        // candidate level
        "discovery_candidate_invalid",
        "discovery_duplicate_candidate",
        "discovery_unknown_namespace",
        "discovery_unknown_tool",
        "discovery_schema_unregistered",
        "discovery_evidence_missing",
        "discovery_project_trust_unverified",
        "discovery_provenance_unverified",
        "extension_manifest_digest_mismatch",
        "discovery_adapter_not_available",
        "discovery_project_mismatch",
        "discovery_capability_claim_mismatch",
        "discovery_capability_not_granted",
        "discovery_operation_out_of_scope",
        "extension_scope_superset_denied",
        "discovery_effect_claim_escalated",
        "discovery_effect_ceiling_exceeded",
    ] {
        assert!(
            DISCOVERY.contains(marker),
            "CAP-30 refusal code missing from discovery.rs: {marker}"
        );
    }
}

#[test]
fn the_non_amplifying_primitives_are_present() {
    for marker in [
        // The intersection is the whole permission-union defense; it must exist and be public.
        "pub fn intersect_extension_scope",
        "requested.intersection(granted)",
        // Staleness is decided by binding the request to the surface digest.
        "surface_digest",
        "request.surface_digest != surface.digest",
        // A failed search is a distinct status, not an empty candidate list.
        "DiscoveryStatus::Unknown",
        "Failed {",
        "retry_required",
        // Namespace admission is fail-closed even though ScopeSet treats NotApplicable as open.
        "ScopeDimension::NotApplicable => false",
        // Effect comes from the registry and the manifest, never from the descriptor.
        "discovery_effect_claim_escalated",
        "registered_effect",
        "side_effecting: registered.side_effecting",
        // The report re-derives rather than believing itself.
        "pub fn validate_against",
        "discovery_report_binding_invalid",
    ] {
        assert!(
            DISCOVERY.contains(marker),
            "CAP-30 invariant missing from discovery.rs: {marker}"
        );
    }
}

#[test]
fn the_search_result_is_a_candidate_and_not_an_authorization() {
    // The single accessor for admitted candidates is gated on the status, so an Unknown or Denied
    // report cannot be read as "everything" or as "nothing, carry on".
    assert!(DISCOVERY.contains("pub fn promotable(&self) -> &[AdmittedDiscoveryCandidate]"));
    assert!(DISCOVERY.contains("if self.status == DiscoveryStatus::Admitted"));
    // Only the report grants a promotion; nothing in the module returns a dispatchable request.
    assert!(!DISCOVERY.contains("AuthorizedCapabilityRequest"));
    assert!(!DISCOVERY.contains("CapabilityRequest"));
    assert!(!DISCOVERY.contains("fn execute"));
}

#[test]
fn the_slice_has_no_side_effect_tokens() {
    // Checked against code, not prose: the module doc is allowed to *say* it does not call
    // ControlPlane or spawn anything, and must not be punished for saying so.
    let code = code_only(DISCOVERY);
    for token in FORBIDDEN_EFFECT_TOKENS {
        assert!(
            !code.contains(token),
            "CAP-30 discovery must stay a read-only contract, found in code: {token}"
        );
    }
    for token in FORBIDDEN_CONTROL_FLOW_TOKENS {
        assert!(
            !code.contains(token),
            "CAP-30 discovery must not reach the broker registry or the ControlPlane, found in code: {token}"
        );
    }
    // Not even a trait method, a callback or a boxed future that a caller could hand an effect to.
    for token in [
        "dyn Fn",
        "BoxFuture",
        "impl Future",
        "async_trait",
        "#[tokio::main]",
        "#[test]",
    ] {
        assert!(
            !DISCOVERY.contains(token),
            "CAP-30 discovery must stay a synchronous pure function, found: {token}"
        );
    }
}

#[test]
fn the_frozen_five_tool_model_surface_is_untouched() {
    // The model-visible tool face is still exactly the five frozen entries. If this assertion ever
    // needs editing, the freeze is being lifted deliberately, not as a side effect of CAP-30.
    for frozen in [
        "`shell`",
        "`apply_patch`",
        "`mcp`",
        "`memory.search`",
        "`memory.write`",
    ] {
        assert!(
            RUNNER_TOOLS.contains(frozen),
            "the frozen model-visible tool {frozen} disappeared from kiana-runner/src/tools.rs"
        );
    }
    // The existing fail-closed fixture for the tool face is still the one that runs.
    assert!(RUNNER_TOOLS.contains("unknown_tool_names_fail_closed_without_alias_or_shell_fallback"));
    // The module must not name a sixth tool it could register as model-visible.
    for smuggled in [
        "tools.push",
        "register_tool",
        "TOOL_SPECS",
        "ToolSpec {",
        "model_schema",
        "wire_name",
    ] {
        assert!(
            !DISCOVERY.contains(smuggled),
            "CAP-30 discovery must not mint model-visible tool schemas, found: {smuggled}"
        );
    }
    // It also must not reach for the kiana-tools legacy registry, which FZ-TOOLS keeps out of the
    // harness.
    assert!(!DISCOVERY.contains("kiana_tools"));
    assert!(!DISCOVERY.contains("kiana_commands"));
    assert!(!DISCOVERY.contains("kiana_tasks"));
    assert!(!DISCOVERY.contains("kiana_types"));
}

#[test]
fn the_permission_union_rule_stays_an_intersection_not_a_union() {
    // The reference rule lives in kiana-policy. This slice has to agree with it, so the guard
    // asserts both sides use intersection semantics for narrowing a child scope.
    assert!(POLICY_GRANT_SCOPE.contains("pub fn intersect(&self, other: &Self)"));
    assert!(POLICY_GRANT_SCOPE.contains("pub fn contains(&self, child: &Self)"));
    assert!(POLICY_GRANT_SCOPE.contains("grant_scope_capability_intersection_empty"));
    // Nothing in discovery may union a granted set into a broader one.
    for widening in [
        "granted.union",
        "granted_capabilities.union",
        "surface.granted_capabilities.union",
        ".extend(",
    ] {
        assert!(
            !DISCOVERY.contains(widening),
            "CAP-30 discovery must never widen a granted scope, found: {widening}"
        );
    }
}

#[test]
fn the_module_is_self_describing_about_its_proof_ceiling() {
    // A read-only contract that does not say it is one is how a source claim becomes a `durable`
    // claim by accident. The module doc has to keep saying what it does not prove.
    for honesty in [
        "read-only contract",
        "candidate, not an authorization",
        "adds no sixth model-visible tool",
        "It is a decision over values a caller",
    ] {
        assert!(
            DISCOVERY.contains(honesty),
            "CAP-30 discovery lost a proof-ceiling statement: {honesty}"
        );
    }
    // The vocabulary the baseline and the fixtures share must not silently fork.
    let codes = DISCOVERY
        .match_indices('"')
        .filter_map(|(index, _)| {
            let rest = &DISCOVERY[index + 1..];
            rest.find('"').map(|end| rest[..end].to_owned())
        })
        .filter(|value| value.starts_with("discovery_") || value.starts_with("extension_"))
        .collect::<BTreeSet<_>>();
    assert!(
        codes.len() >= 23,
        "expected the full CAP-30 refusal vocabulary in source, found {}",
        codes.len()
    );
    for code in &codes {
        assert!(
            code.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "{code} is not stable snake_case"
        );
    }
}
