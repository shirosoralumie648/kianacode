//! H16 的 Domain 调度计划合同测试。
//!
//! 测试把合成的 `ModelToolCall` 直接传给 `plan_tool_batch()`，检查相邻只读调用是否被标成一个
//! 有界组、写入调用是否形成独占屏障，以及 call ID 是否保持模型输出顺序。这里断言的是
//! 计划结构，不会启动并发 worker、调用 Runner/Core 或真实执行 memory 操作。
//! 实现见 `kiana-domain/src/tool_scheduling.rs`；Harness 当前仅校验其输出计划。
use kiana_domain::{plan_tool_batch, ModelToolCall, ToolBatchParallelismCap, ToolSchedulingMode};
use serde_json::json;

/// 构造一个合成的只读 memory.search 调用；ID 同时用作查询文本，便于在计划中辨认顺序。
fn read_call(id: &str) -> ModelToolCall {
    ModelToolCall {
        id: id.to_owned(),
        name: "memory.search".to_owned(),
        arguments: json!({"query":id}),
    }
}

/// 构造一个合成的独占 memory.write 调用；参数只用于构造能力请求形状。
fn write_call(id: &str) -> ModelToolCall {
    ModelToolCall {
        id: id.to_owned(),
        name: "memory.write".to_owned(),
        arguments: json!({"collection":"instance-scratch","text":id,"source":"explicit:h16"}),
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
    let calls = vec![read_call("before"), write_call("write"), read_call("after")];
    let plan = plan_tool_batch(&calls).unwrap();
    assert_eq!(plan.groups.len(), 3);
    assert_eq!(plan.groups[1].mode, ToolSchedulingMode::Exclusive);
    assert_eq!(plan.groups[1].call_ids, ["write"]);
}

#[test]
/// 并行额度取 run、部署、角色三个来源的交集，并受全局上限约束；0 退化为串行而不是停工。
///
/// 这里只验证纯计算：三个来源的声明值由调用方给出，不代表任何真实配额账本已被查询。
fn parallelism_cap_is_the_intersection_of_run_deployment_and_role() {
    assert_eq!(ToolBatchParallelismCap::new(4, 2, 3).effective(), 2);
    assert_eq!(ToolBatchParallelismCap::new(9, 9, 9).effective(), 8);
    assert_eq!(ToolBatchParallelismCap::new(0, 5, 5).effective(), 1);
}

#[test]
/// 组宽度取组自身上限与环境额度的较小值；独占组无论额度多大都只有 1。
///
/// `dispatch_width` 只做数字取交集，不检查授权、不启动线程，也不是资源锁。
fn dispatch_width_intersects_group_and_environment_limits() {
    let reads = vec![read_call("one"), read_call("two"), read_call("three")];
    let plan = plan_tool_batch(&reads).unwrap();
    let read_group = &plan.groups[0];
    assert_eq!(read_group.dispatch_width(ToolBatchParallelismCap::new(2, 2, 2)), 2);
    assert_eq!(read_group.dispatch_width(ToolBatchParallelismCap::new(8, 8, 8)), 4);

    let mixed = vec![read_call("one"), write_call("write"), read_call("two")];
    let mixed = plan_tool_batch(&mixed).unwrap();
    assert_eq!(mixed.groups[1].dispatch_width(ToolBatchParallelismCap::new(8, 8, 8)), 1);
}

#[test]
/// 派发单元按组顺序展开，ordinal 从 0 连续编号且不受独占屏障打断。
///
/// 这是“按 ordinal 形成模型历史”的纯数据前提；实际历史由 Runner 的批次执行器回填。
fn dispatch_units_preserve_source_order_across_barriers() {
    let calls = vec![read_call("a"), read_call("b"), write_call("write"), read_call("c")];
    let plan = plan_tool_batch(&calls).unwrap();
    let units = plan.dispatch_units().unwrap();
    assert_eq!(units.len(), 4);
    let mut ordinals = Vec::new();
    let mut groups = Vec::new();
    for unit in &units {
        ordinals.push(unit.ordinal);
        groups.push(unit.group_id);
    }
    assert_eq!(ordinals, [0, 1, 2, 3]);
    assert_eq!(groups, [1, 1, 2, 3]);
    assert_eq!(units[0].mode, ToolSchedulingMode::ParallelRead);
    assert_eq!(units[2].mode, ToolSchedulingMode::Exclusive);
    assert_eq!(units[2].call_id, "write");
    assert_eq!(units[3].call_id, "c");
}
