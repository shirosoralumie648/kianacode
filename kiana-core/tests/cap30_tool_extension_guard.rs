//! CAP-30 source guard for the existing tool catalog and extension admission boundary.

#[path = "support/source_guard.rs"]
mod source_guard;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        let present = if *marker == "extension_package_sha256" {
            let verify = source_guard::item_source(source, "fn verify(");
            let code = source_guard::compact_code(&verify);
            code.contains("verify_manifest_signature(manifest,&self.trusted_keys)?")
                && code.contains("letmutdigest=Sha256::new()")
                && code.contains("format!(\"{:x}\",digest.finalize())!=manifest.content_hash")
                && code.contains("extension_content_hash_mismatch")
                && code.contains("package_sha256:sha256(&bytes)")
        } else {
            source.contains(marker)
        };
        assert!(present, "CAP-30 {label} marker missing: {marker}");
    }
}

#[test]
fn tool_search_and_extensions_remain_server_owned() {
    let catalog = include_str!("../../kiana-domain/src/tool_catalog.rs");
    let authority = include_str!("../../kiana-domain/src/tool_authority.rs");
    let extensions = include_str!("../../kiana-domain/src/extensions.rs");
    let daemon_extensions =
        &source_guard::production_source(include_str!("../../kiana-daemon/src/extensions.rs"));
    let execution = &[
        source_guard::production_source(include_str!(
            "../../kiana-daemon/src/execution_control.rs"
        )),
        source_guard::production_source(include_str!("../../kiana-domain/src/tool_catalog.rs")),
    ]
    .concat();
    let executor = source_guard::item_source(
        include_str!("../../kiana-daemon/src/execution_control.rs"),
        "async fn handle_read_or_continue(",
    );
    assert!(executor.contains("kiana_domain::search_tool_catalog("));
    assert!(executor.contains("&role.tools"));
    assert!(executor.contains("require_healthy_catalog: true"));
    assert!(!executor.contains("kiana_domain::search_tool_schemas("));
    let limit = source_guard::item_source(
        include_str!("../../kiana-daemon/src/execution_control.rs"),
        "fn tool_search_limit(",
    );
    assert!(limit.contains("usize::try_from(value)"));
    assert!(limit.contains("*value > 0 && *value <= maximum"));
    let skills = include_str!("../../kiana-daemon/src/harness_skills.rs");
    let roles = include_str!("../../kiana-domain/src/roles.rs");
    let ext_fixture = include_str!("../../kiana-domain/tests/ext04_catalog.rs");
    let authority_guard = include_str!("p1_h01_tool_authority_guard.rs");
    let catalog_guard =
        &source_guard::production_source(include_str!("../../kiana-domain/src/tool_authority.rs"));
    let catalog_validate = source_guard::item_source(catalog_guard, "pub fn validate(");
    for binding in [
        "tool.operation != spec.operation",
        "tool.capability != spec.capability.clone()",
        "tool.minimum_risk != spec.risk_policy",
        "tool.side_effecting != spec.side_effecting",
    ] {
        assert!(
            catalog_validate.contains(binding),
            "catalog authority check missing: {binding}"
        );
    }
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
