use kiana_domain::{json_digest, StablePrefix, StablePrefixSegment, TokenAccounting, WireBudget};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!({"value": value}))
}

fn segments() -> Vec<StablePrefixSegment> {
    vec![
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
    ]
}

#[test]
fn provider_request_never_exceeds_declared_budget() {
    let exact = WireBudget::from_final_wire(
        400,
        32,
        64,
        512,
        TokenAccounting::ExactTokenizer {
            input_tokens: 120,
            tokenizer_digest: digest("tokenizer-v1"),
        },
    )
    .unwrap();
    exact.validate().unwrap();
    assert_eq!(exact.input_bytes, 400);
    assert_eq!(exact.total_reserved_tokens, 152);

    assert_eq!(
        WireBudget::from_final_wire(
            400,
            32,
            64,
            151,
            TokenAccounting::ExactTokenizer {
                input_tokens: 120,
                tokenizer_digest: digest("tokenizer-v1"),
            },
        )
        .unwrap_err(),
        "wire_budget_exceeded"
    );
}

#[test]
fn cache_hit_cannot_bypass_revocation() {
    let prefix = StablePrefix::new(
        "builder-default",
        digest("prompt"),
        digest("tools"),
        digest("config-v1"),
        4,
        7,
        segments(),
        digest("dynamic-a"),
    )
    .unwrap();
    prefix
        .cache_hit_allowed(
            "builder-default",
            &digest("prompt"),
            &digest("tools"),
            &digest("config-v1"),
            4,
            7,
        )
        .unwrap();
    assert_eq!(
        prefix
            .cache_hit_allowed(
                "builder-default",
                &digest("prompt"),
                &digest("tools"),
                &digest("config-v1"),
                5,
                7,
            )
            .unwrap_err(),
        "stable_prefix_authority_revoked"
    );
    assert_eq!(
        prefix
            .cache_hit_allowed(
                "builder-default",
                &digest("prompt"),
                &digest("tools"),
                &digest("config-v2"),
                4,
                7,
            )
            .unwrap_err(),
        "stable_prefix_configuration_changed"
    );
}
