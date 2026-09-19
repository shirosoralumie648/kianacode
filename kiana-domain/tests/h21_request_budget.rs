use kiana_domain::{
    json_digest, StablePrefix, StablePrefixSegment, TokenAccounting, WireBudget, WireBudgetInput,
};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!({"value": value}))
}

fn input(accounting: TokenAccounting, limit: u64) -> WireBudgetInput {
    WireBudgetInput {
        system_bytes: 1_024,
        message_bytes: 2_048,
        tool_schema_bytes: 4_096,
        attachment_bytes: 512,
        provider_framing_bytes: 256,
        output_reserved_tokens: 256,
        output_cap_tokens: 512,
        token_limit: limit,
        accounting,
    }
}

#[test]
fn huge_schema_or_system_prompt_exhausts_one_shared_budget() {
    assert_eq!(
        WireBudget::from_input(input(
            TokenAccounting::ConservativeUtf8 {
                bytes_per_token: 2,
                safety_margin_tokens: 64,
            },
            500,
        ))
        .unwrap_err(),
        "wire_budget_exceeded"
    );
    let budget = WireBudget::from_input(input(
        TokenAccounting::ConservativeUtf8 {
            bytes_per_token: 2,
            safety_margin_tokens: 64,
        },
        10_000,
    ))
    .unwrap();
    budget.validate().unwrap();
    assert!(budget.headroom_tokens > 0);
}

#[test]
fn exact_and_conservative_accounting_are_explicit_for_unicode_and_wire_framing() {
    let mut exact = input(TokenAccounting::Exact { input_tokens: 300 }, 1_000);
    exact.system_bytes = "系统🙂".len() as u64;
    let exact_budget = WireBudget::from_input(exact).unwrap();
    assert_eq!(exact_budget.estimated_input_tokens, 300);
    let conservative_budget = WireBudget::from_input(input(
        TokenAccounting::ConservativeUtf8 {
            bytes_per_token: 2,
            safety_margin_tokens: 10,
        },
        10_000,
    ))
    .unwrap();
    assert!(matches!(
        conservative_budget.accounting,
        TokenAccounting::ConservativeUtf8 { .. }
    ));
    assert!(conservative_budget.input_bytes > 0);
}

#[test]
fn stable_prefix_key_excludes_dynamic_suffix_but_changes_on_data_or_catalog_epoch() {
    let segments = vec![
        StablePrefixSegment {
            name: "system".to_owned(),
            order: 0,
            content_digest: digest("system"),
        },
        StablePrefixSegment {
            name: "role".to_owned(),
            order: 100,
            content_digest: digest("role"),
        },
    ];
    let first = StablePrefix::new(
        "builder-default",
        digest("prompt"),
        digest("catalog"),
        digest("config-1"),
        7,
        3,
        segments.clone(),
        digest("time-a"),
    )
    .unwrap();
    let second = StablePrefix::new(
        "builder-default",
        digest("prompt"),
        digest("catalog"),
        digest("config-1"),
        7,
        3,
        segments.clone(),
        digest("time-b"),
    )
    .unwrap();
    assert_eq!(first.prefix_digest, second.prefix_digest);
    assert_eq!(first.cache_key_digest, second.cache_key_digest);
    let revoked = StablePrefix::new(
        "builder-default",
        digest("prompt"),
        digest("catalog-2"),
        digest("config-1"),
        7,
        4,
        segments,
        digest("time-b"),
    )
    .unwrap();
    assert_ne!(first.cache_key_digest, revoked.cache_key_digest);
}
