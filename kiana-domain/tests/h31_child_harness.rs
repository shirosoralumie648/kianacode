use kiana_domain::*;
use std::collections::BTreeSet;

fn budget() -> ChildHarnessBudget {
    ChildHarnessBudget {
        max_tokens: 100,
        max_tool_calls: 4,
        max_effects: 2,
        max_wall_clock_ms: 5_000,
        max_concurrency: 1,
    }
}

fn bounds() -> ChildHarnessBounds {
    ChildHarnessBounds {
        parent_depth: 0,
        max_depth: 2,
        budget: ChildHarnessBudget {
            max_tokens: 200,
            max_tool_calls: 8,
            max_effects: 4,
            max_wall_clock_ms: 10_000,
            max_concurrency: 2,
        },
        allowed_tools: BTreeSet::from(["memory.search".to_owned()]),
        allowed_paths: BTreeSet::from(["src".to_owned()]),
        sandbox: "workspace-write".to_owned(),
    }
}

fn intent() -> ChildHarnessIntent {
    ChildHarnessIntent::new(
        RunId::new(),
        TurnId::new(),
        CellId::new(),
        CellId::new(),
        "packet-child",
        1,
        budget(),
        BTreeSet::from(["memory.search".to_owned()]),
        BTreeSet::from(["src".to_owned()]),
        "read-only",
        vec!["artifact:input".to_owned()],
        "kiana.child-output.v1",
    )
    .unwrap()
}

#[test]
fn child_scope_is_an_intersection_and_uses_typed_intent() {
    let intent = intent();
    intent.validate_against(&bounds()).unwrap();
    assert_eq!(intent.depth, 1);
    assert_eq!(intent.sandbox, "read-only");
    assert!(intent.tools.is_subset(&bounds().allowed_tools));
}

#[test]
fn child_cannot_expand_budget_tools_paths_or_depth() {
    let mut widened = intent();
    widened.budget.max_tokens = 1_000;
    widened.intent_digest = widened.digest();
    assert_eq!(
        widened.validate_against(&bounds()).unwrap_err(),
        "child_harness_scope_widening"
    );

    let mut foreign_tool = intent();
    foreign_tool.tools.insert("shell".to_owned());
    foreign_tool.intent_digest = foreign_tool.digest();
    assert_eq!(
        foreign_tool.validate_against(&bounds()).unwrap_err(),
        "child_harness_scope_widening"
    );

    let mut deep = intent();
    deep.depth = 3;
    deep.intent_digest = deep.digest();
    assert_eq!(
        deep.validate_against(&bounds()).unwrap_err(),
        "child_harness_depth_exceeded"
    );
}

#[test]
fn child_result_returns_refs_without_transcript_and_cancel_unknown_is_explicit() {
    let outcome = ChildHarnessOutcome::new(
        RunId::new(),
        RunId::new(),
        CellId::new(),
        ChildHarnessOutcomeKind::Completed,
        "bounded summary",
        vec!["artifact:child-output".to_owned()],
        vec!["event:child-verified".to_owned()],
    )
    .unwrap();
    outcome.validate().unwrap();
    assert!(!outcome.transcript_forwarded);
    let cancellation = ChildHarnessCancellation::new(
        outcome.parent_run_id,
        outcome.child_run_id,
        "parent_cancelled",
        false,
        false,
    )
    .unwrap();
    assert!(cancellation.result_unknown);
}
