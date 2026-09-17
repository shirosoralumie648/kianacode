# P4-J3-05 终态蒸馏与 lesson candidate 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Distillation contract

终态 `run.completed`、失败、取消和 `result_unknown` 均由现有 ControlPlane/EventLog 终态边界
排入 `memory.distillation_queued`，按 source event 做 at-most-once 去重。运营者显式调用
`memory.distill` 后，队列以 `memory.distillation_claimed`/`started` 绑定一个只读、deny-tools 的
内部 Runner；模型输出必须严格符合 `kiana.memory-distillation.v1`，lesson 的每条引用必须是
源事件中的精确 bounded quote，未知引用、改写、重复或超限均拒绝。

有效输出只产生 `MemoryProposal`：`origin=model`、`admission_state=Candidate`、
`kind=lesson`、collection=`department:<source department>`，并携带 source event/evidence 与
最多三个相似记录。`memory.proposed` 是候选事实，不是 Active/Qualified 知识；`memory.review`
仍是唯一显式 ACL/审批晋升路径。Internal runner、队列重放、claim/finish/failure 和
`result_unknown` 均不执行第二模型循环或直接 `memory.write`。

`run_distillation_lands_as_lesson_candidate` 验证 retain/discard、精确引用和 malformed quote
拒绝；core guard 验证终态触发、队列状态、at-most-once/idempotency、Unknown 和 memory.review
边界。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-domain --test p4_j3_05_distillation --locked -- --test-threads=1
cargo test -p kiana-core --test p4_j3_05_distillation --locked -- --test-threads=1
cargo test -p kiana-domain --test p1_j3_03_memory_proposal --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- 本切片验证的是 bounded Scripted/fixture output 与 EventLog candidate contract；不证明真实
  LLM 质量、生产蒸馏吞吐、跨进程 worker recovery、向量索引或现实业务影响。
- 候选仍需人工 `memory.review`，不能把蒸馏文本、相似记录或模型 verdict 直接当成组织事实；
  data revocation/retention/deletion 继续由 CM/PD/SC 处理。
