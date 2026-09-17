# P4-E-03 五部门会议与决议入部门 RAG 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Department symposium contract

`DepartmentSpec::catalog()` 的 initiating、planning、executing、monitoring、closing 五个
部门都使用同一 `Symposium::department` 合同，固定本部门 chair/attendee 顺序、会议轮数、
Builder 参会边界和 department-specific decision path。`convene_symposium` 仍是唯一会议
执行入口：公开 `DecisionRecord` 写入当前 EventLog 后才派生后续记忆工作，黑板/发言 transcript
不构成授权或第二事实源。

## Decision → memory candidate

会议关闭只把公开决议交给 `derive_memory_proposal(..., "decision")`，再由
`queue_memory_distillation` 生成有 source event、原文引用和相似记录上限的
`MemoryDistillationJob`。消费结果是 `MemoryProposal` 的 `Candidate`，collection 固定为
`department:<department_id>`，不会自动调用 `memory.write` 或直接变成 Active/Qualified。
`memory.review` 仍是显式 operator/approval 路径，按 RoleSpec/MemoryScope ACL、证据和 data
epoch 再决定是否晋升；Builder 的持久层写权限没有因会议扩展而扩大。

`department_resolutions_enter_the_department_memory_layer` 对五个部门逐一构造有效决议源、
验证 Candidate proposal 的 kind/collection/evidence 绑定；core guard 固定事件队列、候选
状态、ACL 和无自动写入边界。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-domain --test p4_e03_department_memory --locked -- --test-threads=1
cargo test -p kiana-core --test p4_e03_department_memory --locked -- --test-threads=1
cargo test -p kiana-core --test p1_e02_symposium --locked -- --test-threads=1
cargo test -p kiana-domain --test p1_j3_03_memory_proposal --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- 本切片证明的是五部门共享合同和决议候选的 source/CI fixture；它不声称跨租户部门 ACL、
  durable Memory/EventStore projector、生产向量索引、外部通知或现实业务结果。
- Candidate proposal 不等于已发布知识；自动蒸馏失败只记录 extraction incident，不改变
  symposium/Company 事实。显式 `memory.review`、审批、撤销和保留/删除传播仍由 CM/PD/SC 后续
  步骤负责。
- 会议中的跨部门联席、Builder 参会例外和新增权限不在本切片内；这些边界继续 fail-closed。
