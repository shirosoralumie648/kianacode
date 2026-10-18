//! AUT-19 拒绝优先夹具：真实 scope/budget/role/TTL/循环 parent、fail-fast 后启动、fan-in 输入
//! 完整性，最后才是“两 child、一个失败、一个跳过、确定性合并”的成功夹具。

use kiana_domain::{
    json_digest, AutomationFanoutChildOutcome, AutomationFanoutChildPlan,
    AutomationFanoutChildState, AutomationFanoutFanInInput, AutomationFanoutParent,
    AutomationFanoutParentState, AutomationFanoutPlan, AutomationFanoutPlanV2, ScopeDimension,
    ScopeLimit, ScopeSet, AUTOMATION_FANOUT_BUDGET_EXCEEDED,
    AUTOMATION_FANOUT_CHILD_ALREADY_STARTED, AUTOMATION_FANOUT_CHILD_COUNT_INVALID,
    AUTOMATION_FANOUT_CHILD_DUPLICATE, AUTOMATION_FANOUT_CHILD_INVALID,
    AUTOMATION_FANOUT_CHILD_NOT_STARTED, AUTOMATION_FANOUT_CHILD_SKIPPED,
    AUTOMATION_FANOUT_CHILD_UNKNOWN, AUTOMATION_FANOUT_CONCURRENCY_EXCEEDED,
    AUTOMATION_FANOUT_CYCLE_DECLARATION_MISMATCH, AUTOMATION_FANOUT_CYCLE_REFUSED,
    AUTOMATION_FANOUT_DEPTH_EXCEEDED, AUTOMATION_FANOUT_EVIDENCE_INVALID,
    AUTOMATION_FANOUT_FAN_IN_DUPLICATE, AUTOMATION_FANOUT_FAN_IN_INCOMPLETE,
    AUTOMATION_FANOUT_FAN_IN_MISSING_CHILD, AUTOMATION_FANOUT_FAN_IN_SEQUENCE_INVALID,
    AUTOMATION_FANOUT_FAN_IN_UNKNOWN_CHILD, AUTOMATION_FANOUT_PARENT_INVALID,
    AUTOMATION_FANOUT_ROLE_WIDENING, AUTOMATION_FANOUT_SCHEMA,
    AUTOMATION_FANOUT_SCOPE_DECLARATION_MISMATCH, AUTOMATION_FANOUT_SCOPE_INVALID,
    AUTOMATION_FANOUT_SCOPE_WIDENING, AUTOMATION_FANOUT_TTL_EXCEEDED,
};
use serde_json::json;

fn digest(seed: &str) -> String {
    json_digest(&json!({ "seed": seed }))
}

fn scope(paths: &[&str]) -> ScopeSet {
    let paths = if paths.is_empty() {
        ScopeDimension::NotApplicable
    } else {
        ScopeDimension::Restricted(paths.iter().map(|value| (*value).to_owned()).collect())
    };
    ScopeSet::new(
        ScopeDimension::Restricted(vec!["shell".to_owned(), "apply_patch".to_owned()]),
        paths,
        ScopeDimension::Restricted(vec!["src".to_owned()]),
        ScopeDimension::Restricted(vec!["localhost".to_owned()]),
        ScopeLimit::Restricted(10),
        ScopeLimit::Restricted(3),
    )
    .expect("fixture scope is valid")
}

fn parent() -> AutomationFanoutParent {
    AutomationFanoutParent {
        parent_execution_id: "parent".to_owned(),
        role: "builder".to_owned(),
        allowed_child_roles: vec!["builder".to_owned(), "reviewer".to_owned()],
        scope: scope(&["src"]),
        budget_units: 10,
        ttl_remaining_ms: 1_000,
        depth: 1,
        max_depth: 3,
        max_concurrency: 2,
        fail_fast: true,
    }
}

fn child(id: &str) -> AutomationFanoutChildPlan {
    AutomationFanoutChildPlan::admit(&parent(), id, "builder", scope(&["src/child"]), 3, 500)
        .expect("fixture child is admitted")
}

fn plan() -> AutomationFanoutPlanV2 {
    AutomationFanoutPlanV2::new(parent(), vec![child("child-a"), child("child-b")])
        .expect("fixture plan is valid")
}

fn input(child_id: &str, sequence: u64) -> AutomationFanoutFanInInput {
    AutomationFanoutFanInInput {
        child_id: child_id.to_owned(),
        sequence,
        evidence_digest: digest(&format!("{child_id}-{sequence}")),
    }
}

#[test]
fn child_scope_widening_is_refused_with_a_stable_code() {
    // 路径维度扩到父范围之外。
    let widening = ScopeSet::new(
        ScopeDimension::Restricted(vec![
            "shell".to_owned(),
            "apply_patch".to_owned(),
            "mcp".to_owned(),
        ]),
        ScopeDimension::Restricted(vec!["src/child".to_owned()]),
        ScopeDimension::Restricted(vec!["src".to_owned()]),
        ScopeDimension::Restricted(vec!["localhost".to_owned()]),
        ScopeLimit::Restricted(10),
        ScopeLimit::Restricted(3),
    )
    .expect("scope itself is valid");
    assert_eq!(
        AutomationFanoutChildPlan::admit(&parent(), "child-a", "builder", widening, 3, 500),
        Err(AUTOMATION_FANOUT_SCOPE_WIDENING)
    );

    // 兄弟路径不落在父路径下。
    let sibling = ScopeSet::new(
        ScopeDimension::Restricted(vec!["shell".to_owned()]),
        ScopeDimension::Restricted(vec!["docs".to_owned()]),
        ScopeDimension::Restricted(vec!["src".to_owned()]),
        ScopeDimension::Restricted(vec!["localhost".to_owned()]),
        ScopeLimit::Restricted(10),
        ScopeLimit::Restricted(3),
    )
    .expect("scope itself is valid");
    assert_eq!(
        AutomationFanoutChildPlan::admit(&parent(), "child-a", "builder", sibling, 3, 500),
        Err(AUTOMATION_FANOUT_SCOPE_WIDENING)
    );

    // 预算/深度上限比父更宽。
    let wider_limit = ScopeSet::new(
        ScopeDimension::Restricted(vec!["shell".to_owned(), "apply_patch".to_owned()]),
        ScopeDimension::Restricted(vec!["src/child".to_owned()]),
        ScopeDimension::Restricted(vec!["src".to_owned()]),
        ScopeDimension::Restricted(vec!["localhost".to_owned()]),
        ScopeLimit::Restricted(11),
        ScopeLimit::Restricted(3),
    )
    .expect("scope itself is valid");
    assert_eq!(
        AutomationFanoutChildPlan::admit(&parent(), "child-a", "builder", wider_limit, 3, 500),
        Err(AUTOMATION_FANOUT_SCOPE_WIDENING)
    );

    // 损坏的 scope（摘要被改写）在比较前就被拒绝。
    let mut corrupt = scope(&["src/child"]);
    corrupt.scope_digest = digest("tampered");
    assert_eq!(
        AutomationFanoutChildPlan::admit(&parent(), "child-a", "builder", corrupt, 3, 500),
        Err(AUTOMATION_FANOUT_SCOPE_INVALID)
    );
}

#[test]
fn budget_role_ttl_and_child_shape_beyond_parent_are_refused() {
    // 单 child 预算超过父预算。
    assert_eq!(
        AutomationFanoutChildPlan::admit(
            &parent(),
            "child-a",
            "builder",
            scope(&["src/child"]),
            11,
            500
        ),
        Err(AUTOMATION_FANOUT_BUDGET_EXCEEDED)
    );
    // 角色不在父允许列表。
    assert_eq!(
        AutomationFanoutChildPlan::admit(
            &parent(),
            "child-a",
            "sponsor",
            scope(&["src/child"]),
            3,
            500
        ),
        Err(AUTOMATION_FANOUT_ROLE_WIDENING)
    );
    // TTL 超过父剩余 TTL。
    assert_eq!(
        AutomationFanoutChildPlan::admit(
            &parent(),
            "child-a",
            "builder",
            scope(&["src/child"]),
            3,
            1_001
        ),
        Err(AUTOMATION_FANOUT_TTL_EXCEEDED)
    );
    // 空 ID / 空 role。
    assert_eq!(
        AutomationFanoutChildPlan::admit(&parent(), " ", "builder", scope(&["src/child"]), 3, 500),
        Err(AUTOMATION_FANOUT_CHILD_INVALID)
    );
    // 父自身无效：深度已到上限。
    let mut too_deep = parent();
    too_deep.depth = 3;
    assert_eq!(too_deep.validate(), Err(AUTOMATION_FANOUT_DEPTH_EXCEEDED));
    too_deep.depth = 1;
    too_deep.max_concurrency = 0;
    assert_eq!(too_deep.validate(), Err(AUTOMATION_FANOUT_PARENT_INVALID));

    // 预算求和（不是只比单个 child）超父预算。
    let mut second = child("child-b");
    second.budget_units = 8;
    assert_eq!(
        AutomationFanoutPlanV2::new(parent(), vec![child("child-a"), second]),
        Err(AUTOMATION_FANOUT_BUDGET_EXCEEDED)
    );
    // 预算求和溢出也必须拒绝，而不是饱和成上限。
    let mut huge_parent = parent();
    huge_parent.budget_units = u64::MAX;
    let mut first = AutomationFanoutChildPlan::admit(
        &huge_parent,
        "child-a",
        "builder",
        scope(&["src/child"]),
        u64::MAX,
        500,
    )
    .expect("a single max-budget child is within the max parent budget");
    let mut second = first.clone();
    second.child_execution_id = "child-b".to_owned();
    first.budget_units = u64::MAX;
    assert_eq!(
        AutomationFanoutPlanV2::new(huge_parent, vec![first, second]),
        Err(AUTOMATION_FANOUT_BUDGET_EXCEEDED)
    );
    // 并发上限在账本运行时强制；这里只证明三个 child 的计划仍可接纳。
    let mut wide = parent();
    wide.max_concurrency = 3;
    assert!(AutomationFanoutPlanV2::new(
        wide,
        vec![child("child-a"), child("child-b"), child("child-c")]
    )
    .is_ok());
}

#[test]
fn cycle_parent_and_duplicate_children_are_refused() {
    assert_eq!(
        AutomationFanoutChildPlan::admit(
            &parent(),
            "parent",
            "builder",
            scope(&["src/child"]),
            3,
            500
        ),
        Err(AUTOMATION_FANOUT_CYCLE_REFUSED)
    );
    assert_eq!(
        AutomationFanoutPlanV2::new(parent(), vec![child("child-a"), child("child-a")]),
        Err(AUTOMATION_FANOUT_CHILD_DUPLICATE)
    );
    assert_eq!(
        AutomationFanoutPlanV2::new(parent(), Vec::new()),
        Err(AUTOMATION_FANOUT_CHILD_COUNT_INVALID)
    );

    // 反序列化后的计划同样不能绕过校验（字段公开）。
    let mut value = serde_json::to_value(plan()).expect("plan serializes");
    value["children"][0]["child_execution_id"] = json!("parent");
    value["plan_digest"] = json!(json_digest(&json!({
        "schema": value["schema"],
        "parent": value["parent"],
        "children": value["children"],
    })));
    let tampered: AutomationFanoutPlanV2 =
        serde_json::from_value(value).expect("shape is readable");
    assert_eq!(tampered.validate(), Err(AUTOMATION_FANOUT_CYCLE_REFUSED));
}

#[test]
fn start_after_fail_fast_is_refused_and_unknown_or_unstarted_children_are_refused() {
    let mut ledger = plan().admit().expect("plan admits");
    assert_eq!(ledger.parent_state(), AutomationFanoutParentState::Admitted);
    // 未启动 child 不能 observe。
    assert_eq!(
        ledger.observe_child(
            "child-a",
            AutomationFanoutChildOutcome::Succeeded {
                output_digest: digest("a")
            }
        ),
        Err(AUTOMATION_FANOUT_CHILD_NOT_STARTED)
    );
    // 未知 child。
    assert_eq!(
        ledger.start_child("ghost"),
        Err(AUTOMATION_FANOUT_CHILD_UNKNOWN)
    );
    // started 之后 fail-fast 之前，另一个 child 仍可启动。
    assert!(ledger.start_child("child-a").is_ok());
    assert_eq!(
        ledger.start_child("child-a"),
        Err(AUTOMATION_FANOUT_CHILD_ALREADY_STARTED)
    );
    // child-a 失败 => child-b 被标 Skipped。
    ledger
        .observe_child(
            "child-a",
            AutomationFanoutChildOutcome::Failed {
                failure_code: "automation_fanout_child_failed".to_owned(),
            },
        )
        .expect("failure is recorded");
    assert_eq!(
        ledger.child_state("child-b"),
        Some(AutomationFanoutChildState::Skipped)
    );
    // 已跳过的 child 不能晚到启动。
    assert_eq!(
        ledger.start_child("child-b"),
        Err(AUTOMATION_FANOUT_CHILD_SKIPPED)
    );
    assert_eq!(ledger.parent_state(), AutomationFanoutParentState::Failed);
}

#[test]
fn started_children_are_bounded_by_parent_concurrency() {
    let mut limited = parent();
    limited.max_concurrency = 1;
    let plan = AutomationFanoutPlanV2::new(limited, vec![child("child-a"), child("child-b")])
        .expect("two children with concurrency 1 is still a valid plan");
    let mut ledger = plan.admit().expect("plan admits");
    assert!(ledger.start_child("child-a").is_ok());
    assert_eq!(
        ledger.start_child("child-b"),
        Err(AUTOMATION_FANOUT_CONCURRENCY_EXCEEDED)
    );
}

#[test]
fn fan_in_refuses_duplicate_unknown_missing_and_incomplete_inputs() {
    let mut ledger = plan().admit().expect("plan admits");
    assert!(ledger.start_child("child-a").is_ok());
    assert!(ledger.start_child("child-b").is_ok());
    // 尚未终态。
    assert_eq!(
        ledger.merge(&[input("child-a", 1), input("child-b", 1)]),
        Err(AUTOMATION_FANOUT_FAN_IN_INCOMPLETE)
    );
    ledger
        .observe_child(
            "child-a",
            AutomationFanoutChildOutcome::Succeeded {
                output_digest: digest("a"),
            },
        )
        .expect("success is recorded");
    ledger
        .observe_child(
            "child-b",
            AutomationFanoutChildOutcome::Succeeded {
                output_digest: digest("b"),
            },
        )
        .expect("success is recorded");
    assert_eq!(
        ledger.merge(&[
            input("child-a", 1),
            input("child-a", 1),
            input("child-b", 1)
        ]),
        Err(AUTOMATION_FANOUT_FAN_IN_DUPLICATE)
    );
    assert_eq!(
        ledger.merge(&[input("child-a", 1), input("child-b", 1), input("ghost", 1)]),
        Err(AUTOMATION_FANOUT_FAN_IN_UNKNOWN_CHILD)
    );
    assert_eq!(
        ledger.merge(&[input("child-a", 1)]),
        Err(AUTOMATION_FANOUT_FAN_IN_MISSING_CHILD)
    );
    assert_eq!(
        ledger.merge(&[input("child-a", 0), input("child-b", 1)]),
        Err(AUTOMATION_FANOUT_FAN_IN_SEQUENCE_INVALID)
    );
    let mut bad_digest = input("child-b", 1);
    bad_digest.evidence_digest = "sha256:xyz".to_owned();
    assert_eq!(
        ledger.merge(&[input("child-a", 1), bad_digest]),
        Err(AUTOMATION_FANOUT_EVIDENCE_INVALID)
    );
    // 无效输出摘要不能写成成功事实。
    let mut fresh = plan().admit().expect("plan admits");
    assert!(fresh.start_child("child-a").is_ok());
    assert_eq!(
        fresh.observe_child(
            "child-a",
            AutomationFanoutChildOutcome::Succeeded {
                output_digest: "not-a-digest".to_owned()
            }
        ),
        Err(AUTOMATION_FANOUT_EVIDENCE_INVALID)
    );
}

#[test]
fn legacy_plan_declarations_are_checked_against_real_scopes() {
    let mut value = AutomationFanoutPlan {
        schema: AUTOMATION_FANOUT_SCHEMA.to_owned(),
        parent_execution_id: "parent".to_owned(),
        child_execution_ids: vec!["child-a".to_owned()],
        max_depth: 3,
        depth: 1,
        max_concurrency: 2,
        requested_concurrency: 1,
        parent_budget_units: 10,
        child_budget_units: 4,
        ttl_ms: 100,
        parent_ttl_remaining_ms: 200,
        child_scope_is_subset: true,
        parent_id_in_children: false,
        fail_fast: true,
        fan_in_required: true,
        plan_digest: String::new(),
    };
    let legacy_digest = |value: &AutomationFanoutPlan| {
        json_digest(&json!({
            "schema": value.schema,
            "parent_execution_id": value.parent_execution_id,
            "child_execution_ids": value.child_execution_ids,
            "max_depth": value.max_depth,
            "depth": value.depth,
            "max_concurrency": value.max_concurrency,
            "requested_concurrency": value.requested_concurrency,
            "parent_budget_units": value.parent_budget_units,
            "child_budget_units": value.child_budget_units,
            "ttl_ms": value.ttl_ms,
            "parent_ttl_remaining_ms": value.parent_ttl_remaining_ms,
            "child_scope_is_subset": value.child_scope_is_subset,
            "parent_id_in_children": value.parent_id_in_children,
            "fail_fast": value.fail_fast,
            "fan_in_required": value.fan_in_required,
        }))
    };
    value.plan_digest = legacy_digest(&value);
    // 自报 true，但真实 child scope 更宽。
    assert_eq!(
        value.validate_with_scopes(&scope(&["src"]), &scope(&["docs"])),
        Err(AUTOMATION_FANOUT_SCOPE_WIDENING)
    );
    // 自报 true 且真实 scope 确实是子集时通过。
    assert!(value
        .validate_with_scopes(&scope(&["src"]), &scope(&["src/child"]))
        .is_ok());
    // 自报 false，但真实 scope 确实是子集：声明与事实不符，validate() 也拒绝。
    value.child_scope_is_subset = false;
    value.plan_digest = legacy_digest(&value);
    assert_eq!(value.validate(), Err("automation_fanout_plan_invalid"));
    assert_eq!(
        value.validate_with_scopes(&scope(&["src"]), &scope(&["src/child"])),
        Err(AUTOMATION_FANOUT_SCOPE_DECLARATION_MISMATCH)
    );
    // 自报未成环，但 child 列表实际包含 parent：validate() 直接拒绝循环。
    value.child_scope_is_subset = true;
    value.child_execution_ids = vec!["parent".to_owned()];
    value.plan_digest = legacy_digest(&value);
    assert_eq!(value.validate(), Err(AUTOMATION_FANOUT_CYCLE_REFUSED));
    assert_eq!(
        value.validate_with_scopes(&scope(&["src"]), &scope(&["src/child"])),
        Err(AUTOMATION_FANOUT_CYCLE_DECLARATION_MISMATCH)
    );
    // 从真实值重算声明后，布尔值与真实事实一致，摘要同步。
    let mut derived = AutomationFanoutPlan {
        parent_execution_id: "parent".to_owned(),
        child_execution_ids: vec!["child-a".to_owned()],
        parent_id_in_children: true,
        ..value
    };
    derived.plan_digest = legacy_digest(&derived);
    derived
        .derive_declarations(&scope(&["src"]), &scope(&["src/child"]))
        .expect("derivation succeeds");
    assert!(derived.child_scope_is_subset);
    assert!(!derived.parent_id_in_children);
    assert!(derived.validate().is_ok());
    assert!(derived.plan_digest.starts_with("sha256:"));
}

#[test]
fn admitted_two_child_plan_fails_one_skips_the_other_and_merges_deterministically() {
    let mut ledger = plan().admit().expect("plan admits");
    let admission = ledger.admission();
    assert_eq!(admission.child_ids, vec!["child-a", "child-b"]);
    assert!(admission.child_scope_is_subset);
    assert!(!admission.parent_id_in_children);

    ledger.start_child("child-b").expect("child-b starts first");
    ledger.start_child("child-a").expect("child-a starts");
    ledger
        .observe_child(
            "child-b",
            AutomationFanoutChildOutcome::Failed {
                failure_code: "automation_fanout_child_failed".to_owned(),
            },
        )
        .expect("child-b fails");
    // child-a 已启动，仍然可以记录自己的结果；新 child 则会被跳过。
    ledger
        .observe_child(
            "child-a",
            AutomationFanoutChildOutcome::Succeeded {
                output_digest: digest("child-a-output"),
            },
        )
        .expect("child-a succeeds");

    assert_eq!(
        ledger.child_state("child-a"),
        Some(AutomationFanoutChildState::Succeeded)
    );
    assert_eq!(
        ledger.child_state("child-b"),
        Some(AutomationFanoutChildState::Failed)
    );
    assert_eq!(ledger.parent_state(), AutomationFanoutParentState::Failed);

    let inputs = vec![
        input("child-b", 2),
        input("child-a", 1),
        input("child-b", 1),
    ];
    let first = ledger.merge(&inputs).expect("terminal children merge");
    let second = ledger
        .merge(&[
            input("child-a", 1),
            input("child-b", 1),
            input("child-b", 2),
        ])
        .expect("input order does not matter");
    assert_eq!(first, second, "merge must be stable under input order");
    assert_eq!(
        first
            .entries
            .iter()
            .map(|entry| (entry.child_id.as_str(), entry.sequence))
            .collect::<Vec<_>>(),
        vec![("child-a", 1), ("child-b", 1), ("child-b", 2)]
    );
    assert_eq!(first.parent_state, AutomationFanoutParentState::Failed);
    assert!(first.merge_digest.starts_with("sha256:"));
}

#[test]
fn fail_fast_skip_order_is_lexicographic_and_never_starts() {
    let mut wide = parent();
    wide.max_concurrency = 3;
    let plan = AutomationFanoutPlanV2::new(
        wide,
        vec![child("child-c"), child("child-a"), child("child-b")],
    )
    .expect("three children fit");
    let mut ledger = plan.admit().expect("plan admits");
    ledger.start_child("child-c").expect("child-c starts");
    // 未启动的 child-a/child-b 按字典序跳过（不是计划顺序）。
    ledger
        .observe_child(
            "child-c",
            AutomationFanoutChildOutcome::Failed {
                failure_code: "automation_fanout_child_failed".to_owned(),
            },
        )
        .expect("failure is recorded");
    let skipped: Vec<&str> = ledger
        .facts()
        .iter()
        .filter_map(|fact| match fact {
            kiana_domain::AutomationFanoutFact::Skipped { child_id, .. } => Some(child_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(skipped, vec!["child-a", "child-b"]);
    assert_eq!(
        ledger.start_child("child-a"),
        Err(AUTOMATION_FANOUT_CHILD_SKIPPED)
    );
    assert_eq!(
        ledger.start_child("child-b"),
        Err(AUTOMATION_FANOUT_CHILD_SKIPPED)
    );
}
