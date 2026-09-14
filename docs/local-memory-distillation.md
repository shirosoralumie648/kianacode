# 本地 Memory 蒸馏

run 到终态后，ControlPlane 把已脱敏的终态和同一轮少量证据写入 EventLog
`memory_distillation` 流。会议仅排入已发布的决议摘要。排队不调用模型，也不写入
qualified Memory。内部蒸馏 run 不再排队。

通过现有 versioned `command` 入口发送 `memory.distill`：

```json
{"name":"memory.distill","arguments":{"action":"list"}}
```

```json
{"name":"memory.distill","arguments":{"action":"consume","job_id":"<source-event-uuid>"}}
```

`consume` 是默认 action；省略 `job_id` 时只领取当前主体、项目、角色和部门可见的
最早一项 queued 任务。必须使用已信任项目和本地人工入口；Cell 不能消费。
重复请求 ID 不会再领取另一项任务。该命令沿通常 `ControlPlane::start_run_with_id`
→ `KianaHarness` → 已配置 provider 路由调用模型，沿用原角色的 model profile，
没有第二个执行循环。

默认仅排队。组合根的可信环境变量 `KIANA_MEMORY_DISTILL_AUTO=1` 或 `true` 可启用
普通用户 run 完成后消费一项，默认关闭。自动消费不会改变原 run 的终态、响应或
Receipt，但启用后会增加一次模型调用的等待时间和 usage。环境变量不是项目 skill
配置。模型未配置、输出不合法或预算不足时留下失败事实，不声称完成蒸馏。

每项任务按 source event ID 去重，并依次记录：

```text
memory.distillation_queued
  → memory.distillation_claimed
  → memory.distillation_started
  → memory.proposed | memory.distillation_completed | memory.distillation_failed
```

claim 通过 EventStore CAS 绑定 internal request/run/session、主体、项目、原角色和
部门。仅当持久 claim、确切证据 prompt、空历史和 read-only sandbox 都匹配时，
core 才允许开始并将步数限制为 1。内部 session 前缀不授予权限；policy 对该前缀
拒绝所有 capability，继续和恢复也必须拒绝。五个模型工具名称没有变化。
蒸馏系统指令是可信 Product 段；项目、用户 skill 和已安装 extension 的上下文不参与
这一轮，以减少无关指令干扰。模型输入中证据始终只是低信任数据。

证据最多 12 项、总计 16 KiB，另带原 run 已授权 Memory 检索结果中的最多 3 条旧记录。
模型必须返回纯 JSON，不允许 Markdown fence 或额外字段：

```json
{
  "schema":"kiana.memory-distillation.v1",
  "verdict":"retain",
  "reason":"对后续同类任务有用",
  "lessons":[{
    "kind":"lesson",
    "text":"一个可复用的经验",
    "evidence":[{"event_id":"<supplied-event-uuid>","quote":"证据中的原文"}]
  }]
}
```

`discard` 必须带空 `lessons`；`retain` 必须包含 1–4 条，且每条 kind 等于任务的
`lesson` 或 `decision`。每条最多 6 个引用，ID 必须来自服务端证据包，quote 必须是
对应证据的精确非空子串。系统记录真实 provider/model、prompt hash、usage 和模型轮
event ID。候选限定原部门 collection、origin=model、admission=candidate、ADD 建议；
模型不能指定权限、collection、批准状态或其他记录的删除/覆盖。

`memory.proposed` 既是 candidate 事实，也是该任务的原子完成标记。候选仍需人工经
`memory.review` 的 `accept_proposal` 和原有 T1 审批入库，未批准候选不可作为
qualified Memory 检索结果。引文校验只能证明引用来源，不能证明模型经验正确。

领取后发生崩溃时不自动重跑付费模型；`list` 将未决 claim/started 标记为
`result_unknown`。若内部 run 已持久完成，可显式执行：

```json
{"name":"memory.distill","arguments":{"action":"finalize","job_id":"<source-event-uuid>"}}
```

`finalize` 只读取已存在的模型终态并校验输出，完全不调用模型。仍在运行或终态缺失时
拒绝，保留未决状态。该行为优先避免重复支出，不把缺失事实当作成功。

本次仅完成源码和离线编译检查，未新增或运行测试，也没有 live provider 证明。
