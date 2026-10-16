//! H16 的 Domain 调度计划合同测试。
//!
//! 测试把合成的 `ModelToolCall` 直接传给 `plan_tool_batch()`，检查相邻只读调用是否被标成一个
//! 有界组、写入调用是否形成独占屏障，以及 call ID 是否保持模型输出顺序。这里断言的是
//! 计划结构，不会启动并发 worker、调用 Runner/Core 或真实执行 memory 操作。
//! 实现见 `kiana-domain/src/tool_scheduling.rs`；Harness 当前仅校验其输出计划。
use kiana_domain::{plan_tool_batch, ModelToolCall, ToolSchedulingMode};
use serde_json::json;

/// 构造一个合成的只读 memory.search 调用；ID 同时用作查询文本，便于在计划中辨认顺序。
fn read_call(id: &str) -> ModelToolCall {
    ModelToolCall {
        id: id.to_owned(),
        name: "memory.search".to_owned(),
        arguments: json!({"query":id}),
    }
}

#[test]
/// 连续只读调用被规划到同一有界 parallel-read 组，组内 call ID 仍按输入次序排列。
///
/// `overlap` 是计划允许的调度类别，不是此测试测量到真实并发执行的证明；断言只检查 plan。
fn independent_reads_overlap_but_history_is_source_ordered() {
    let calls = vec![read_call("one"), read_call("two"), read_call("three")];
    let plan = plan_tool_batch(&calls).unwrap();
    assert_eq!(plan.groups.len(), 1);
    assert_eq!(plan.groups[0].mode, ToolSchedulingMode::ParallelRead);
    assert_eq!(plan.groups[0].max_parallelism, 4);
    assert_eq!(plan.groups[0].call_ids, ["one", "two", "three"]);
}

#[test]
/// memory.write 在前后只读组之间形成独占屏障，且自身作为单调用组。
///
/// 此处验证相邻组的分类和顺序；没有写入实际 memory collection，也没有经过 ControlPlane。
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
