//! H30 检索入口共享 Domain ContextPlan 的静态源码守卫。
//!
//! 测试仅对 Domain、Query 和 Core 源文件做 `include_str!` 字符串断言，以约束候选合同与
//! 入口边界的关键 marker；它不编译候选、不执行权限决策，也不证明 adapter 已从生产路由调用。

#[test]
fn retrieval_adapters_enter_one_context_plan_and_not_a_second_authority_path() {
    let domain = include_str!("../../kiana-domain/src/retrieval_context.rs");
    let plan = include_str!("../../kiana-domain/src/context_plan.rs");
    let query = include_str!("../../kiana-query/src/lib.rs");
    let adapter = include_str!("../../kiana-query/src/context_inputs.rs");
    let core = include_str!("../src/context_query.rs");
    // 这些 marker 被允许分布在两个 Domain 文件中，不要求单个函数具备完整调用链。
    for marker in [
        "RetrievalCandidate",
        "SourceSnapshot",
        "permission_scope",
        "acl_digest",
        "Freshness",
        "evidence_refs",
        "as_context_candidate",
        "ContextPlan::compile",
    ] {
        assert!(
            domain.contains(marker) || plan.contains(marker),
            "missing H30 marker: {marker}"
        );
    }
    // 后续断言检查源码文本中的导出/调用边界词，不替代编译期依赖审计。
    assert!(query.contains("context_inputs"));
    assert!(adapter.contains("RetrievalCandidate"));
    assert!(core.contains("authorize_and_execute"));
    assert!(!domain.contains("PromptAuthority::Product"));
    assert!(!adapter.contains("CapabilityGrant"));
}
