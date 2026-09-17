use kiana_domain::{plan_tool_batch, ModelToolCall, ToolSchedulingMode};
use serde_json::json;

fn read_call(id: &str) -> ModelToolCall {
    ModelToolCall {
        id: id.to_owned(),
        name: "memory.search".to_owned(),
        arguments: json!({"query":id}),
    }
}

#[test]
fn independent_reads_overlap_but_history_is_source_ordered() {
    let calls = vec![read_call("one"), read_call("two"), read_call("three")];
    let plan = plan_tool_batch(&calls).unwrap();
    assert_eq!(plan.groups.len(), 1);
    assert_eq!(plan.groups[0].mode, ToolSchedulingMode::ParallelRead);
    assert_eq!(plan.groups[0].max_parallelism, 4);
    assert_eq!(plan.groups[0].call_ids, ["one", "two", "three"]);
}

#[test]
fn exclusive_calls_form_barriers_around_read_groups() {
    let calls = vec![
        read_call("before"),
        ModelToolCall {
            id: "write".to_owned(),
            name: "memory.write".to_owned(),
            arguments: json!({
                "collection":"instance-scratch",
                "text":"write",
                "source":"explicit:h16"
            }),
        },
        read_call("after"),
    ];
    let plan = plan_tool_batch(&calls).unwrap();
    assert_eq!(plan.groups.len(), 3);
    assert_eq!(plan.groups[1].mode, ToolSchedulingMode::Exclusive);
    assert_eq!(plan.groups[1].call_ids, ["write"]);
}
