use kiana_domain::TokenBudget;

#[test]
fn tool_schemas_count_toward_the_budget() {
    let with_schema = TokenBudget::new(10, 20, 100, 10, 500);
    let without_schema = TokenBudget::new(10, 20, 0, 10, 500);
    assert!(with_schema.tool_schemas > without_schema.tool_schemas);
    assert!(with_schema.total > without_schema.total);

    let exhausted = TokenBudget::new(10, 20, 100, 10, with_schema.total - 1);
    assert_eq!(exhausted.validate(), Err("context_budget_exceeded"));
    assert_eq!(
        TokenBudget::new(0, 0, 0, 0, 0).validate(),
        Err("context_budget_unavailable")
    );
}

#[test]
fn budget_accounting_is_explicit_about_its_conservative_bound() {
    let budget = TokenBudget::new(1, 2, 3, 4, 1_000);
    assert_eq!(budget.accounting, "utf8_wire_bytes_with_framing_reserve");
    assert_eq!(budget.messages, 257);
    assert_eq!(budget.system_prompt, 34);
    assert_eq!(budget.tool_schemas, 259);
    assert_eq!(budget.reserved_output, 4);
    assert_eq!(budget.total, 554);
}
