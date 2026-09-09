# Kiana 执行路线图（逐步指引）

> 这份文档只回答一个问题：**接下来每一步做什么、谁做、怎么算做完。**
>
> 当前事实以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本文只排顺序和验收口径，**不定义新规范**。
> 规范是什么、现在是什么、计划是什么三者的权威顺序见 [`README.md`](README.md) §3。
> 缺口清单的来源：`reference-agent-audit/00-unified-agent-flow.md` 的 P0/P1/P2 backlog，以及 2026-09-10 对当前代码的逐条复核（见每步的"现状"）。

---

## 0. 怎么用这份文档

- **一次只做一步。** 做完、CI 绿、证据块落账，再开下一步。
- 每步的节奏固定：**Codex 写实现 → Claude 审 diff + 编译检查 + 提交推送 → GitHub CI 跑全量门禁 → 证据块进 `CURRENT_STATUS.md`**。
- 每步的「验收」写的是**必须存在的测试名**和**必须保持的既有断言**。删测试、放宽断言、跳过用例，都不算完成。
- **CI 红就不是做完。** 本机只跑 `make test-fast` 和针对性测试，全量交给 CI。
- 冻结项（`AGENTS.md` §7）任何一步都不许打开：HTTP MCP、支付/出行/打车/订票/IoT、企业租户与远程执行、自由多 agent 消息总线。

---

## 1. 为什么是这个顺序

三条理由，按权重排：

1. **先补事实源。** 恢复、对账、审批续跑、UI 重连，全都建立在"事件账本能重建状态"之上。账本缺字段，后面每一样都要打补丁。
2. **再补安全边界。** 取消语义、工具授权、资源预算，是"出错时不许撒谎"的底线，且不依赖界面。
3. **最后补体验。** 事件协议、context 组装、界面一致性，只有前两者稳定后做才不会返工。

---

## 2. 阶段总览

| 阶段 | 主题 | 步数 | 依赖 | 状态 |
|---|---|---|---|---|
| **P0** | 基线复位 | 5 | — | 4/5 完成 |
| **P1** | 账本与恢复 | 4 | — | 未开始 |
| **P2** | 取消契约 | 4 | 与 P1 部分重叠 | 未开始 |
| **P3** | 工具 authority | 4 | — | 未开始 |
| **P4** | 有界循环与预算 | 3 | — | 1/3 排队中 |
| **P5** | 事件协议与并发所有权 | 4 | P1 | 未开始 |
| **P6** | context 组装 | 3 | — | 未开始 |
| **P7** | 架构决策：第二条执行循环 | 1 | — | 待决策 |
| **P8** | 参考审计补完（文档） | 2 | — | 未开始 |

---

## 3. P0 — 基线复位（已完成 4/5）

目的是把流式那条线收干净，让 CI 重新成为可信门禁。

### 0.1 账本粒度修正 ✅

- **做了什么**：流式增量按轮次聚合，一条 `run.delta` 不再按 provider 分块落账。
- **证据**：`CURRENT_STATUS.md`「Streamed delta ledger granularity correction evidence」。
- **提交**：`e9df8b4`。

### 0.2 不完整流 fail-closed + CI 挂起修正 ✅

- **做了什么**：流在终止事件前结束 → `provider_stream_incomplete`，不再返回"成功且空白"；4 个测试 mock 改成会回 SSE。
- **为什么紧急**：`make test-fast` 不含 entrypoints 目标，导致 SDK/bridge 的挂起在本地看不见，CI 从 `3b6f31a` 起连续 9 小时每跑必挂 6 小时。
- **证据**：`CURRENT_STATUS.md`「Incomplete provider stream fail-closed and CI hang correction evidence」。
- **提交**：`3b65af2`（CI `34372274823` 16 分钟全绿）。

### 0.3 文档与已落地证据对齐 ✅

- **做了什么**：README/USER 不再写"不声称 token 流式"，`CURRENT_STATUS.md` 头部改成绑定 HEAD 快照，验收清单勾掉已被证据覆盖的负向项。
- **提交**：`99237ad`。

### 0.4 命令行默认开启流式 ✅

- **做了什么**：`kiana run` 普通路径默认流式，`--no-stream` / `--json` 关闭；`--stream` 与互斥模式仍报 `stream_requires_run`。
- **依据**：`streaming-unfreeze-plan.md` §8 的书面决定（live 证据落地后默认开启）。
- **提交**：`e2b15c1`。

### 0.5 断线/重连负向路径 🔄

- **做什么**：SSE 订阅到进行中的 run 时显式发 `stream_gap`；前端不得把不完整的轮次标成完成；重连不重放已送 delta。
- **验收**：`web_sse_reconnect_emits_stream_gap_without_replaying_delta_items`。
- **完成定义**：`streaming-unfreeze-plan.md` §6 的最后一条负向项可以打勾。

---

## 4. P1 — 账本与恢复（P0 级价值，最先做）

**为什么最先**：`CURRENT_STATUS.md` 把"跨进程完整 resume"记为 `deferred`；`kiana-core/src/sessions.rs` 的绑定只写内存，重启后 `continue_run` / `cancel_run` / `read_receipt` 一律 `session_not_found`。这是与成熟参考差距最大的一块。

### 1.1 session→run 绑定从事件账本重建

- **现状**：绑定只在 `kiana-core/src/sessions.rs:48-62` 的内存 map；`run.authorized` 事件（`lifecycle.rs:105-121`）的字段恰好覆盖 binding 所需字段，可以纯投影得到。
- **做什么**：内存未命中时从账本只读回读并重建绑定；账本里确实没有时仍 fail-closed 返回 `session_not_found`。
- **验收**：`fresh_control_plane_rebuilds_session_binding_from_ledger`、`missing_session_still_fails_closed`、`rebuild_does_not_replay_completed_capability`。
- **风险**：不得因"从磁盘读"放宽 owner/role/path/approval 授权。

### 1.2 账本记录可重放 history 所需的字段

- **现状**：账本不记 user prompt，也不记 provider 的 `tool_call_id`（`lifecycle.rs:40-54`、`capabilities.rs:94-107`），所以 model-visible history 无法重建。
- **做什么**：新增 `run.prompt` / `run.tool_call` / `run.tool_result` 事件 kind；全部过既有 `redact_event_value`。
- **验收**：`resume_rebuilds_model_visible_history_from_ledger`、`prompt_and_tool_arguments_are_redacted_in_the_ledger`。
- **风险**：新事件会增加 append 次数；必须沿用 0.1 的合并语义，保持"每轮一条 `run.delta`"。

### 1.3 `resume_run` 与协议入口

- **做什么**：`kiana-core` 新增 `resume_run`，复用同一条 `drive_run`；`kiana-protocol` 加 **additive** 的 `ResumeRequest`，`PROTOCOL_SCHEMA` 不动。
- **验收**：`resume_after_torn_tail_discards_partial_turn`、`fresh_process_resume_reconstructs_pending_approval`、`legacy_client_ignores_resume_fields`。
- **风险**：不得新增第二条执行循环；必须走 `DaemonHost → ControlPlane → RunnerPort`。

### 1.4 网页重启后列出历史会话（只读）

- **来源**：`.codex/TASKS.md` 的可选任务。
- **做什么**：Web 侧从事件账本列出历史 session（只读），不写新状态。
- **验收**：`web_lists_persisted_sessions_after_restart`。
- **依赖**：1.1 完成后才有意义。

---

## 5. P2 — 取消契约

**为什么**：SEC-08（取消不得返回 completed）、SEC-09（无法确认 → `result_unknown`）。现在三套取消机制并存：core 用 `watch<bool>`、runner 用 `Mutex<Option<String>>`、legacy 入口第三套 abort watch。

### 2.1 统一 cancellation token 与状态词表

- **做什么**：新增 run 级 `RunCancellationState { Accepted, Active, Queued, Cancelling, Cancelled, Terminal }` 与转移表；`ExecutionStatus` 补 `Queued` / `Cancelling`。
- **验收**：`cancel_transitions_are_total_and_irreversible`、`cancelling_is_recorded_before_terminal`。
- **风险**：新增中间态会改变 `run.cancelled` 的时序，必须保住"每 run 恰好一条终态"。

### 2.2 排空已启动工作 + 为未启动工具合成结果

- **现状**：`KianaHarness::cancel` 在 in-flight 时只发 `Failed{cancelled:}` 就返回，`pending_tools` 和 inbox 直接丢弃。
- **参考形状**：`reference/crush/internal/agent/agent.go:432-465`（给被丢弃的 queued call 补发 terminal）。
- **验收**：`cancel_drains_queued_tool_calls_with_replay_safe_results`。

### 2.3 进程组确认与 `stop_confirmed`

- **现状**：取消路径不校验进程是否真的停了（只有 timeout 路径校验）；MCP stdio 只在 Drop 里 `start_kill`。
- **做什么**：取消走 `terminate_process_group` 并返回 `stop_confirmed`；未确认即 `result_unknown`。
- **验收**：`cancel_confirms_shell_process_group_stopped`、`unconfirmed_stop_returns_result_unknown`。

### 2.4 取消竞态负向证据

- **验收**：`cancel_race_never_produces_wrong_completion`、`late_tool_result_after_cancel_is_discarded`。
- **注意**：必须保留现有 `daemon_host::cancelling_mid_stream_never_completes_or_emits_a_late_delta` 的语义。

---

## 6. P3 — 工具 authority 集中化

**为什么**：SEC-01/INP-01。现在权威分散在 5 处（schema 目录、名字→能力映射、角色工具表、策略边界、执行 handler），新增工具要改多处。

### 3.1 `ToolSpec` registry（单一真源）

- **做什么**：`kiana-domain` 新增 `tool_authority` 模块，定义 `ToolSpec{name, aliases, capability, operation, risk_policy, side_effecting, schema}` 与 `TOOL_SPECS`；runner/policy 改为由它驱动。
- **验收**：`tool_authority_covers_every_model_visible_tool`、`canonical_tool_name_matches_role_tool_lists`。
- **硬约束**：**不新增模型可见工具**，工具数量保持 5 个。

### 3.2 参数 schema 校验（映射期拒绝）

- **现状**：`tool_schemas()` 只发给 provider，`call.arguments` 从不校验；缺字段变 `Value::Null`，直到 handler 才报 `harness_argument_required`。
- **做什么**：新增 `validate_tool_arguments`，失败返回 `invalid_arguments:<tool>:<field>`。
- **验收**：`malformed_arguments_are_rejected_before_capability_mapping`。
- **风险**：`additionalProperties` 不能默认禁止，否则老 cassette 会被误拒。

### 3.3 路径 containment 共享实现

- **现状**：策略层只对 `apply_patch` 提取路径，shell 全靠 bwrap/workdir，两套规则无共享实现。
- **验收**：`path_containment_is_shared_by_every_side_effecting_tool`。

### 3.4 稳定错误码枚举

- **做什么**：`CapabilityErrorCode` enum + `CapabilityResult::failure_code()`，替换字符串错误。
- **验收**：`path_escape_uses_stable_error_code`。

---

## 7. P4 — 有界循环与预算（SEC-12）

### 4.1 连续重复工具调用检测 🔄

- **现状**：`pending_tools` 逐张弹出，无去重、无连续相同调用计数。
- **做什么**：同一工具 + 相同 arguments 连续重复达阈值 → `repeated_tool_call:<name>` fail-closed；阈值进 `RuntimeConfig`；不同参数不误伤。
- **验收**：`repeated_identical_tool_call_fails_closed`、`different_arguments_are_not_blocked`、`alternating_tools_reset_the_counter`。

### 4.2 `max_steps` 按角色生效

- **现状**：`RuntimeConfig` / `with_max_steps` 存在但产品路径从不调用（恒 32 步）；`RoleSpec.max_steps` 不生效。
- **做什么**：把角色的 `max_steps` 接到 run；默认值不变。
- **验收**：`role_max_steps_reaches_the_harness`。

### 4.3 run 级 wall-time 预算

- **做什么**：run 级超时 → fail-closed，写 `run.budget_exceeded`。
- **验收**：`run_wall_time_budget_fails_closed`。

---

## 8. P5 — 事件协议与并发所有权

### 5.1 wire 上加 `sequence` / `epoch`（additive）

- **现状**：`RunStreamEnvelope` 只有 `schema` + `event`，没有单调序号；客户端无法排序或识别重启后的旧事件。
- **验收**：`run_stream_sequence_is_monotonic`、`legacy_client_ignores_sequence_and_epoch`。

### 5.2 补齐事件种类

- **现状**：只投影 `Delta`，`CapabilityRequested` / 审批 / usage 全丢。
- **做什么**：additive 增加 `Usage` / `ToolCall` / `ApprovalRequested` / `Error`。
- **验收**：`tool_lifecycle_events_are_projected`。

### 5.3 terminal 必达 + replay cursor

- **现状**：无订阅者时不发 terminal、投递后通道删除；SSE 无 `Last-Event-ID`。
- **验收**：`terminal_is_replayed_to_late_subscriber`、`web_sse_resumes_from_last_event_id`。

### 5.4 控制面单活跃 turn 守卫

- **现状**：Web 有 per-session 守卫，控制面没有（`remember_session` 直接覆盖旧绑定）。
- **验收**：`control_plane_rejects_a_second_active_turn`。

---

## 9. P6 — context 组装

### 6.1 类型化区段 + provenance

- **现状**：只有压缩预算；system/history/tools/user 没有名字、顺序或来源。
- **做什么**：`PromptSection{name, order, text}` + `render_prompt()`；固定区段 `system.role` / `system.skills` / `runtime.workspace` / `history` / `tools` / `user`。
- **验收**：`context_sections_render_with_provenance`。

### 6.2 预算覆盖 tool schemas 与 system prompt

- **现状**：`history_tokens` 只累加 `message.text`，工具 schema 和 system prompt 不计入，估算系统性偏小。
- **验收**：`tool_schemas_count_toward_the_budget`、`oversized_history_is_truncated_not_silently_dropped`。

### 6.3 角色 prompt 接线

- **现状**：`RoleSpec.prompt` 只用于沙箱判定；provider 的 system 恒为 Builder 文案。
- **验收**：`role_prompt_reaches_the_provider_system_message`。

---

## 10. P7 — 架构决策：第二条执行循环（需要产品主人拍板）

- **现状**：`kiana-entrypoints/src/runner.rs:633/1095` 直连 `execute_tool_calls_with_permission_handler`，绕过 broker 与 ControlPlane；`kiana-tools/src/agent.rs:2937` 生成的团队 agent 脚本会带 `--resident-teammate`，因此这条路径**可达**。
- **问题**：这违反仓库自己的硬边界（"Keep model/tool execution brokered"）。
- **两条路，二选一**：
  - (a) 迁移到 `DaemonHost` 脊柱；
  - (b) 明确标记为 legacy 并冻结（写进 `AGENTS.md` §7 与 `coding-pack-matrix.md`），同时确保它不进产品帮助文案。
- **产出**：一份书面决定 + 对应文档更新。**不要在没有决定前动代码。**

---

## 11. P8 — 参考审计补完（文档，可并行）

- **8.1** 重审 `codex` / `deepseek-harness` / `goose`（`reference-agent-audit/README.md` 已列出顺序）。
- **8.2** 新增 `27-grok-build.md`；补审计 temporal-sdk-python / container-use / beads / graphiti / mem0 / a2a / Archon-Knowledge / spec-kit / OpenSpec。

---

## 12. 每步的完成定义（不许放宽）

一步算完成，必须同时满足：

1. 代码在 `master` 上，CI 的 `release-smoke` **全绿**；
2. 新增负向测试的名字出现在本文件对应条目的"验收"里，且**先红后绿**（有观察记录）；
3. `CURRENT_STATUS.md` 有绑定源码快照的证据块（命令、退出码、限制）；
4. 没有删除、放宽或跳过任何既有断言；
5. 冻结项没有被打开。

**不算完成**：代码存在但没进产品路径、单测绿但 CI 红、一次本机成功就写成 `durable`/`live`、把 `target` 改写成 `implemented`。
