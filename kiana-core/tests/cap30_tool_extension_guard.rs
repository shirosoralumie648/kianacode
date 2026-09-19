//! CAP-30 source guard for the existing tool catalog and extension admission boundary.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-30 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn tool_search_and_extensions_remain_server_owned() {
    let catalog = include_str!("../../kiana-domain/src/tool_catalog.rs");
    let authority = include_str!("../../kiana-domain/src/tool_authority.rs");
    let extensions = include_str!("../../kiana-domain/src/extensions.rs");
    let daemon_extensions = include_str!("../../kiana-daemon/src/extensions.rs");
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");
    let skills = include_str!("../../kiana-daemon/src/harness_skills.rs");
    let roles = include_str!("../../kiana-domain/src/roles.rs");
    let ext_fixture = include_str!("../../kiana-domain/tests/ext04_catalog.rs");
    let authority_guard = include_str!("p1_h01_tool_authority_guard.rs");
    let catalog_guard = include_str!("p4_j7_10_catalog_guard.rs");
    let baseline = include_str!("../../docs/roadmap/cap30-tool-extension-baseline.md");

    require(
        catalog,
        &[
            "tool_schemas",
            "validate_tool_arguments",
            "TOOL_SCHEMA_DIALECT",
            "validate_schema_contract",
        ],
        "tool schema catalog",
    );
    require(
        authority,
        &[
            "ToolCatalogSnapshot",
            "ToolDescriptor",
            "tool_catalog_digest",
            "execution_mode",
            "replay_class",
            "resources",
            "max_parallelism",
        ],
        "authority snapshot",
    );
    require(
        extensions,
        &[
            "ExtensionManifest",
            "license",
            "effect",
            "required_capabilities",
            "content_hash",
            "capability_diff",
            "ExtensionExecutionContract",
        ],
        "extension contract",
    );
    require(
        daemon_extensions,
        &[
            "trusted_keys",
            "verify_manifest_signature",
            "extension_package_sha256",
            "extension_version_content_conflict",
            "extension_package_revoked",
            "extension_publisher_change_denied",
            "extension_rollback_snapshot_missing",
            "set_extension_admission",
        ],
        "extension admission",
    );
    require(
        execution,
        &[
            "tool.search",
            "search_tool_schemas",
            "search_tool_catalog",
            "ToolSearchOptions",
            "estimated_context_tokens",
            "tool_search_catalog_version_mismatch",
            "tool_search_context_budget_exceeded",
            "ranked_token_bounded",
            "catalog_digest",
            "does_not_grant_execution",
            "role.tools",
        ],
        "tool search executor",
    );
    require(
        skills,
        &["project_trusted", "trust", "PromptAuthority"],
        "skill trust",
    );
    require(roles, &["tools", "path_allow", "role_id"], "role policy");
    require(
        ext_fixture,
        &[
            "ExtensionCatalog",
            "duplicate_identity_shadowed",
            "order_hooks",
        ],
        "extension catalog fixture",
    );
    require(
        authority_guard,
        &[
            "validate_tool_authority",
            "TOOL_SPECS",
            "single_tool_authority",
        ],
        "authority regression",
    );
    require(
        catalog_guard,
        &[
            "ToolCatalogSnapshot",
            "catalog_digest",
            "tool_catalog_authority_drift",
        ],
        "catalog regression",
    );
    require(
        baseline,
        &[
            "tool_search_never_returns_invisible_or_untrusted_descriptors",
            "extension_cannot_self_grant_or_replace_builtin_binding",
            "read_only_extension_write_is_denied_at_executor",
        ],
        "CAP-30 acceptance card",
    );
    assert!(baseline.contains("partial"));
    assert!(!daemon_extensions.contains("CapabilityBroker::new"));
}
