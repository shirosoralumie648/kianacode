use kiana_domain::{search_tool_catalog, tool_schemas, ToolSearchOptions, TOOL_CATALOG_VERSION};

fn all_tools() -> Vec<String> {
    tool_schemas()
        .into_iter()
        .filter_map(|schema| schema["name"].as_str().map(ToOwned::to_owned))
        .collect()
}

#[test]
fn selected_schemas_are_versioned_and_token_budgeted() {
    let response = search_tool_catalog(
        "memory",
        &all_tools(),
        4,
        128 * 1024,
        &ToolSearchOptions {
            catalog_version: TOOL_CATALOG_VERSION,
            max_context_tokens: 8_192,
            require_healthy_catalog: true,
            replay_safe_only: true,
        },
    )
    .unwrap();
    assert!(response
        .tools
        .iter()
        .all(|tool| tool["name"] == "memory.search"));
    assert!(response.selected_schema_bytes > 0);
    assert_eq!(
        response.estimated_context_tokens,
        response.selected_schema_bytes.div_ceil(4)
    );
    assert!(response.does_not_grant_execution);
}

#[test]
fn stale_catalog_and_context_budget_are_rejected() {
    let mut stale = ToolSearchOptions::default();
    stale.catalog_version = kiana_domain::SchemaVersion::new(9, 9);
    assert_eq!(
        search_tool_catalog("shell", &all_tools(), 1, 128 * 1024, &stale).unwrap_err(),
        "tool_search_catalog_version_mismatch"
    );

    let mut too_small = ToolSearchOptions::default();
    too_small.max_context_tokens = 1;
    assert_eq!(
        search_tool_catalog("shell", &all_tools(), 1, 128 * 1024, &too_small).unwrap_err(),
        "tool_search_context_budget_exceeded"
    );
}
