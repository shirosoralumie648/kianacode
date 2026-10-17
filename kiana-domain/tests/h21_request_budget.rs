use kiana_domain::{
    json_digest, tool_catalog_hash, tool_schemas, StablePrefix, StablePrefixSegment,
    TokenAccounting, WireBudget, WireBudgetInput, REQUEST_BUDGET_COVERAGE_ITEMS,
};
use serde_json::Value;
use std::collections::BTreeSet;

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

/// 边界输入：CJK、代码与 emoji 都走同一份显式标记的保守本地估计；公式（bytes-per-token +
/// 余量）被逐样本钉住，不把 4 bytes/token 或固定 framing 当作任意模型的硬上界。
#[test]
fn conservative_estimate_is_explicit_for_cjk_code_and_emoji_boundaries() {
    let samples = [
        "中文预算边界：系统段、历史内容块与工具 schema 共用同一份预算。",
        "fn main() { let 预算 = \"schema\"; println!(\"{}\", 预算.len()); }",
        "🙂🧑‍💻🀄️🫠",
    ];
    for sample in samples {
        let mut boundary = input(
            TokenAccounting::ConservativeUtf8 {
                bytes_per_token: 2,
                safety_margin_tokens: 128,
            },
            10_000,
        );
        boundary.system_bytes = sample.len() as u64;
        boundary.message_bytes = 0;
        boundary.tool_schema_bytes = 0;
        boundary.attachment_bytes = 0;
        boundary.provider_framing_bytes = 0;
        let budget = WireBudget::from_input(boundary).unwrap();
        budget.validate().unwrap();
        assert!(matches!(
            budget.accounting,
            TokenAccounting::ConservativeUtf8 { .. }
        ));
        let bytes = sample.len() as u64;
        assert_eq!(
            budget.estimated_input_tokens,
            (bytes + 1) / 2 + 128,
            "the local estimate keeps its declared bytes-per-token and safety margin"
        );
    }
}

/// 长工具 schema 与其它部分共用同一份预算：超出即拒绝，在放行时才要求 headroom。
#[test]
fn long_tool_schema_exhausts_the_same_request_budget_before_sending() {
    let mut boundary = input(
        TokenAccounting::ConservativeUtf8 {
            bytes_per_token: 1,
            safety_margin_tokens: 4_096,
        },
        8_192,
    );
    boundary.system_bytes = 0;
    boundary.message_bytes = 0;
    boundary.tool_schema_bytes = 65_536;
    boundary.attachment_bytes = 0;
    boundary.provider_framing_bytes = 0;
    boundary.output_reserved_tokens = 1_024;
    boundary.output_cap_tokens = 2_048;
    assert_eq!(
        WireBudget::from_input(boundary.clone()).unwrap_err(),
        "wire_budget_exceeded"
    );

    boundary.token_limit = 80_000;
    let budget = WireBudget::from_input(boundary).unwrap();
    assert_eq!(budget.input_bytes, 65_536);
    assert_eq!(budget.total_reserved_tokens, 65_536 + 4_096 + 1_024);
    assert!(budget.headroom_tokens > 0);
    assert!(budget.coverage_items().contains(&"tool_schemas"));
}

/// 精确计数能力：提供 tokenizer 摘要时按声明计数（不重新按字节折算），超限同样拒绝，
/// 摘要非法或计数为 0 时 fail closed。
#[test]
fn exact_tokenizer_accounting_is_declared_digest_bound_and_still_bounded() {
    let exact = WireBudget::from_input(input(
        TokenAccounting::ExactTokenizer {
            input_tokens: 700,
            tokenizer_digest: digest("tokenizer-v1"),
        },
        1_000,
    ))
    .unwrap();
    assert_eq!(exact.estimated_input_tokens, 700);
    assert_eq!(exact.total_reserved_tokens, 700 + 256);
    assert_eq!(
        WireBudget::from_input(input(
            TokenAccounting::ExactTokenizer {
                input_tokens: 745,
                tokenizer_digest: digest("tokenizer-v1"),
            },
            1_000,
        ))
        .unwrap_err(),
        "wire_budget_exceeded"
    );
    assert_eq!(
        WireBudget::from_input(input(
            TokenAccounting::ExactTokenizer {
                input_tokens: 100,
                tokenizer_digest: "tokenizer-v1".to_owned(),
            },
            10_000,
        ))
        .unwrap_err(),
        "wire_budget_tokenizer_digest_invalid"
    );
    assert_eq!(
        WireBudget::from_input(input(TokenAccounting::Exact { input_tokens: 0 }, 10_000))
            .unwrap_err(),
        "wire_budget_exact_tokens_invalid"
    );
}

/// 预算覆盖项与身份：固定顺序的七个组成部分；同一输入得到同一预算摘要，输入字节或输出
/// 预留变化即换身份，旧预算不能复用在新请求上。
#[test]
fn budget_coverage_items_are_fixed_and_identical_inputs_share_one_digest() {
    let conservative = || {
        input(
            TokenAccounting::ConservativeUtf8 {
                bytes_per_token: 4,
                safety_margin_tokens: 64,
            },
            10_000,
        )
    };
    let first = WireBudget::from_input(conservative()).unwrap();
    let second = WireBudget::from_input(conservative()).unwrap();
    assert_eq!(first.budget_digest, second.budget_digest);
    assert_eq!(first.coverage_items(), REQUEST_BUDGET_COVERAGE_ITEMS);
    assert_eq!(
        first.coverage_items(),
        [
            "system",
            "role",
            "history_content_blocks",
            "tool_schemas",
            "attachments",
            "provider_framing",
            "output_reserve",
        ]
    );
    let mut changed_bytes = conservative();
    changed_bytes.message_bytes += 1;
    assert_ne!(
        first.budget_digest,
        WireBudget::from_input(changed_bytes).unwrap().budget_digest
    );
    let mut changed_reserve = conservative();
    changed_reserve.output_reserved_tokens += 1;
    assert_ne!(
        first.budget_digest,
        WireBudget::from_input(changed_reserve)
            .unwrap()
            .budget_digest
    );
}

/// 先拒绝：数据删除/内容撤销（data epoch 变化）与权限收紧（authority epoch 变化）优先于
/// 可见稳定段比较；随后才允许同身份复用。
#[test]
fn cache_hit_cannot_bypass_data_epoch_revocation() {
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
    let reusable = StablePrefix::new(
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
    assert!(reusable.reuse_for(&reusable.clone()).is_ok());

    let data_revoked = StablePrefix::new(
        "builder-default",
        digest("prompt"),
        digest("catalog"),
        digest("config-1"),
        7,
        4,
        segments.clone(),
        digest("time-a"),
    )
    .unwrap();
    assert_eq!(reusable.prefix_digest, data_revoked.prefix_digest);
    assert_ne!(reusable.cache_key_digest, data_revoked.cache_key_digest);
    assert_eq!(
        reusable.reuse_for(&data_revoked).unwrap_err(),
        "stable_prefix_data_epoch_changed"
    );

    let authority_revoked = StablePrefix::new(
        "builder-default",
        digest("prompt"),
        digest("catalog"),
        digest("config-1"),
        8,
        3,
        segments,
        digest("time-a"),
    )
    .unwrap();
    assert_eq!(
        reusable.reuse_for(&authority_revoked).unwrap_err(),
        "stable_prefix_authority_revoked"
    );
}

/// 稳定前缀段顺序与唯一性：按声明优先级排序，重名条目拒绝。
#[test]
fn stable_prefix_segments_are_ordered_by_declared_priority_and_names_are_unique() {
    let unsorted = vec![
        StablePrefixSegment {
            name: "role".to_owned(),
            order: 100,
            content_digest: digest("role"),
        },
        StablePrefixSegment {
            name: "system".to_owned(),
            order: 0,
            content_digest: digest("system"),
        },
    ];
    let prefix = StablePrefix::new(
        "builder-default",
        digest("prompt"),
        digest("catalog"),
        digest("config-1"),
        1,
        1,
        unsorted,
        digest("time"),
    )
    .unwrap();
    assert_eq!(
        prefix
            .segments
            .iter()
            .map(|segment| segment.name.as_str())
            .collect::<Vec<_>>(),
        ["system", "role"]
    );
    let mut duplicate = prefix.segments.clone();
    duplicate.push(duplicate[0].clone());
    assert_eq!(
        StablePrefix::new(
            "builder-default",
            digest("prompt"),
            digest("catalog"),
            digest("config-1"),
            1,
            1,
            duplicate,
            digest("time"),
        )
        .unwrap_err(),
        "stable_prefix_segment_duplicate"
    );
}

/// 工具顺序属于缓存身份：五个模型可见工具的目录顺序是确定的，而目录指纹覆盖精确顺序，
/// 因此重排工具不是同一份稳定前缀（不是集合比较）。
#[test]
fn fixed_tool_order_is_stable_and_pinned_by_the_catalog_hash() {
    let tools = tool_schemas();
    assert_eq!(
        tools.len(),
        5,
        "the model-visible tool surface stays at the frozen five"
    );
    assert_eq!(
        tool_schemas(),
        tools,
        "the catalog order is deterministic, not per-call shuffled"
    );
    let names = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "apply_patch",
            "mcp",
            "memory.search",
            "memory.write",
            "shell",
        ])
    );
    let mut reordered = tools.clone();
    reordered.reverse();
    assert_ne!(
        tool_catalog_hash(&tools),
        tool_catalog_hash(&reordered),
        "the pinned catalog hash covers the exact order"
    );
}
