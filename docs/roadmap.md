# Kiana 执行路线图（进度 + 指引）

> **一屏看进度** → §1 状态表。**看某步具体做什么** → §4 起的详细卡。**你想加东西** → §11 追加区。
> 当前事实以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；本文只排顺序、记进度、写验收口径，**不定义新规范**。
> 缺口来源：`reference-agent-audit/00-unified-agent-flow.md` 的 P0/P1/P2 backlog + 2026-09-10 对当前代码的逐条复核。

---

## 0. 怎么用这份文档

**状态图例**

| 标记 | 含义 |
|---|---|
| ✅ | 完成：代码在 `master`、CI 全绿、有证据块 |
| 🔄 | 进行中：Codex 在写，或已推送等 CI |
| ⏳ | 待办：已排进队列，还没开工 |
| ❓ | 待你拍板：不该由 Codex 或我替你决定 |
| 🚫 | 冻结：明确不做（见 `AGENTS.md` §7） |

**更新规则（每完成一块必须做）**

1. 改 §1 状态表里那一行的状态、提交哈希、CI run id；
2. 在 §3 变更日志加一行（日期 + 做了什么 + 提交）；
3. 该步的验收测试名如果和当初计划的不一样，改详细卡里的"验收"；
4. `CURRENT_STATUS.md` 里加对应证据块（绑定源码快照 + 命令 + 退出码 + 限制）。

**你加新条目**：在 §11 追加区按模板写一行，我会把它拆成 Codex 任务排进队列，做完再回填状态。

**固定节奏**：`Codex 写实现 → 我审 diff + 编译检查 + 提交推送 → GitHub CI 跑全量门禁 → 回填本文件 + 证据块`。
一次只做一步；CI 红就不是做完。

---

## 1. 进度总览

| 步骤 | 状态 | 提交 | CI | 验收测试名 |
|---|---|---|---|---|
| **0.1** 账本粒度修正 | ✅ | `e9df8b4` | — | `contiguous_stream_deltas_become_one_durable_run_delta` |
| **0.2** 不完整流 fail-closed + CI 挂起修正 | ✅ | `3b65af2` | `34372274823` ✅ | `native_streaming_without_a_terminal_event_fails_closed` |
| **0.3** 文档与证据对齐 | ✅ | `99237ad` | `34372547148` ✅ | —（文档） |
| **0.4** 命令行默认开启流式 | ✅ | `e2b15c1` + `d704add` | `34375303757` | `run_streams_each_delta_by_default_before_terminal_receipt` |
| **0.5** 断线/重连负向路径 | 🔄 | `8b2aecb` | 等 CI | `web_sse_reconnect_emits_stream_gap_without_replaying_delta_items` |
| **1.1** session 绑定从账本重建 | 🔄 | Codex 进行中 | — | `fresh_control_plane_rebuilds_session_binding_from_ledger` |
| **1.2** 账本记录可重放 history 字段 | ⏳ | — | — | `resume_rebuilds_model_visible_history_from_ledger` |
| **1.3** `resume_run` 与协议入口 | ⏳ | — | — | `fresh_process_resume_reconstructs_pending_approval` |
| **1.4** 网页重启后列出历史会话 | ⏳ | — | — | `web_lists_persisted_sessions_after_restart` |
| **2.1** 统一 cancellation token 与状态词表 | ⏳ | — | — | `cancel_transitions_are_total_and_irreversible` |
| **2.2** 排空已启动工作 + 合成未启动结果 | ⏳ | — | — | `cancel_drains_queued_tool_calls_with_replay_safe_results` |
| **2.3** 进程组确认与 `stop_confirmed` | ⏳ | — | — | `cancel_confirms_shell_process_group_stopped` |
| **2.4** 取消竞态负向证据 | ⏳ | — | — | `cancel_race_never_produces_wrong_completion` |
| **3.1** `ToolSpec` registry | ⏳ | — | — | `tool_authority_covers_every_model_visible_tool` |
| **3.2** 参数 schema 校验 | ⏳ | — | — | `malformed_arguments_are_rejected_before_capability_mapping` |
| **3.3** 路径 containment 共享实现 | ⏳ | — | — | `path_containment_is_shared_by_every_side_effecting_tool` |
| **3.4** 稳定错误码枚举 | ⏳ | — | — | `path_escape_uses_stable_error_code` |
| **4.1** 连续重复工具调用检测 | 🔄 | `d973ff6` | 等 CI | `third_consecutive_identical_tool_call_fails_closed` |
| **4.2** `max_steps` 按角色生效 | ⏳ | — | — | `role_max_steps_reaches_the_harness` |
| **4.3** run 级 wall-time 预算 | ⏳ | — | — | `run_wall_time_budget_fails_closed` |
| **5.1** wire 加 `sequence`/`epoch` | ⏳ | — | — | `run_stream_sequence_is_monotonic` |
| **5.2** 补齐事件种类 | ⏳ | — | — | `tool_lifecycle_events_are_projected` |
| **5.3** terminal 必达 + replay cursor | ⏳ | — | — | `terminal_is_replayed_to_late_subscriber` |
| **5.4** 控制面单活跃 turn 守卫 | ⏳ | — | — | `control_plane_rejects_a_second_active_turn` |
| **6.1** 类型化区段 + provenance | ⏳ | — | — | `context_sections_render_with_provenance` |
| **6.2** 预算覆盖 tool schemas | ⏳ | — | — | `tool_schemas_count_toward_the_budget` |
| **6.3** 角色 prompt 接线 | ⏳ | — | — | `role_prompt_reaches_the_provider_system_message` |
| **7.1** 第二条执行循环的处置 | ❓ | — | — | 需要你拍板（§10） |
| **8.1** 重审 codex / deepseek-harness / goose | ⏳ | — | — | —（文档） |
| **8.2** 新增 grok-build + 补审计 9 项 | ⏳ | — | — | —（文档） |

---

## 2. 当前在飞

| 事项 | 位置 | 卡在哪 |
|---|---|---|
| Codex 任务 3：session 绑定从账本重建 | `kiana-core`（sessions/receipts/lifecycle + 测试） | Codex 正在写 |
| CI `34375303757` | `d704add` | `release-smoke` 跑着 |
| CI `8b2aecb`（断线重连） | — | 排队 |
| CI `d973ff6`（doom-loop） | — | 排队 |

---

## 3. 变更日志

| 日期 | 做了什么 | 提交 |
|---|---|---|
| 2026-09-09 | 流式增量按轮次聚合落账，修掉 129 条 `run.delta` 的写放大 | `e9df8b4` |
| 2026-09-09 | 不完整流 fail-closed（`provider_stream_incomplete`）+ 4 个 mock 改回 SSE，修掉 CI 挂 9 小时 | `3b65af2` |
| 2026-09-09 | README/USER/CURRENT_STATUS 与已落地证据对齐 | `99237ad` |
| 2026-09-09 | 命令行默认开启流式 + `--no-stream` 关闭开关 | `e2b15c1` |
| 2026-09-10 | 本文件建立 | `26c69ef` |
| 2026-09-10 | SSE 断线发 `stream_gap`、不完整的轮次不标记完成 | `8b2aecb` |
| 2026-09-10 | 连续重复工具调用 fail-closed（`repeated_tool_call:<name>`） | `d973ff6` |
| 2026-09-10 | 修正被默认翻转影响的 `cli_run` 测试（显式 `--no-stream` + 默认路径覆盖） | `d704add` |

---

## 4. P0 — 基线复位

目的是把流式那条线收干净，让 CI 重新成为可信门禁。

### 0.1 账本粒度修正 ✅

- **做了什么**：流式增量按轮次聚合，一条 `run.delta` 不再按 provider 分块落账。
- **证据**：`CURRENT_STATUS.md`「Streamed delta ledger granularity correction evidence」。

### 0.2 不完整流 fail-closed + CI 挂起修正 ✅

- **做了什么**：流在终止事件前结束 → `provider_stream_incomplete`；4 个测试 mock 改成会回 SSE。
- **为什么紧急**：`make test-fast` 不含 entrypoints 目标，挂起在本地看不见，CI 从 `3b6f31a` 起连续 9 小时每跑必挂 6 小时。

### 0.3 文档与证据对齐 ✅

- **做了什么**：README/USER 不再写"不声称 token 流式"；`CURRENT_STATUS.md` 头部改成绑定 HEAD 快照；勾掉已被证据覆盖的负向项。

### 0.4 命令行默认开启流式 ✅

- **做了什么**：`kiana run` 普通路径默认流式；`--no-stream` / `--json` 关闭；`--stream` 与互斥模式仍报 `stream_requires_run`。
- **代价**：第一次推送 CI 就抓到 `cli_run` 的默认行为断言失效，用 `d704add` 修正（显式 `--no-stream` + 新增默认路径覆盖）。

### 0.5 断线/重连负向路径 🔄

- **做什么**：SSE 订阅到进行中的 run 时显式发 `stream_gap`；前端不得把不完整的轮次标成完成；重连不重放已送 delta。
- **完成定义**：`streaming-unfreeze-plan.md` §6 最后一条负向项可以打勾。

---

## 5. P1 — 账本与恢复

**为什么最先**：`CURRENT_STATUS.md` 把"跨进程完整 resume"记为 `deferred`；session 绑定只写内存，重启后 `continue_run` / `cancel_run` / `read_receipt` 一律 `session_not_found`。

### 1.1 session→run 绑定从事件账本重建 🔄

- **现状**：绑定只在 `kiana-core/src/sessions.rs:48-62` 的内存 map；`run.authorized`（`lifecycle.rs:105-121`）字段恰好覆盖 binding。
- **做什么**：内存未命中时从账本只读回读并重建；账本里确实没有时仍 fail-closed。
- **风险**：不得因"从磁盘读"放宽 owner/role/path/approval 授权。

### 1.2 账本记录可重放 history 所需的字段

- **现状**：账本不记 user prompt，也不记 provider 的 `tool_call_id`（`lifecycle.rs:40-54`、`capabilities.rs:94-107`）。
- **做什么**：新增 `run.prompt` / `run.tool_call` / `run.tool_result` 事件 kind；全部过 `redact_event_value`。
- **风险**：新事件会增加 append 次数，必须沿用 0.1 的合并语义。

### 1.3 `resume_run` 与协议入口

- **做什么**：`kiana-core` 新增 `resume_run`，复用同一条 `drive_run`；`kiana-protocol` 加 **additive** 的 `ResumeRequest`，`PROTOCOL_SCHEMA` 不动。

### 1.4 网页重启后列出历史会话（只读）

- **来源**：`.codex/TASKS.md` 可选任务。
- **依赖**：1.1 完成后才有意义。

---

## 6. P2 — 取消契约

**为什么**：SEC-08 / SEC-09。现在三套取消机制并存：core 用 `watch<bool>`、runner 用 `Mutex<Option<String>>`、legacy 入口第三套。

### 2.1 统一 cancellation token 与状态词表

- **做什么**：`RunCancellationState { Accepted, Active, Queued, Cancelling, Cancelled, Terminal }` + 转移表；`ExecutionStatus` 补 `Queued` / `Cancelling`。
- **风险**：新增中间态会改变 `run.cancelled` 时序，必须保住"每 run 恰好一条终态"。

### 2.2 排空已启动工作 + 为未启动工具合成结果

- **现状**：`KianaHarness::cancel` 在 in-flight 时只发 `Failed{cancelled:}` 就返回，`pending_tools` 与 inbox 直接丢弃。
- **参考形状**：`reference/crush/internal/agent/agent.go:432-465`。

### 2.3 进程组确认与 `stop_confirmed`

- **现状**：取消路径不校验进程是否真的停了；MCP stdio 只在 Drop 里 `start_kill`。

### 2.4 取消竞态负向证据

- **注意**：必须保留 `daemon_host::cancelling_mid_stream_never_completes_or_emits_a_late_delta` 的语义。

---

## 7. P3 — 工具 authority 集中化

**为什么**：SEC-01 / INP-01。权威分散在 5 处，新增工具要改多处。

### 3.1 `ToolSpec` registry（单一真源）

- **做什么**：`kiana-domain` 新增 `tool_authority` 模块（`ToolSpec{name, aliases, capability, operation, risk_policy, side_effecting, schema}` + `TOOL_SPECS`）。
- **硬约束**：**不新增模型可见工具**，保持 5 个。

### 3.2 参数 schema 校验（映射期拒绝）

- **现状**：`tool_schemas()` 只发给 provider，`call.arguments` 从不校验。
- **风险**：`additionalProperties` 不能默认禁止，否则老 cassette 被误拒。

### 3.3 路径 containment 共享实现

- **现状**：策略层只对 `apply_patch` 提取路径，shell 全靠 bwrap/workdir。

### 3.4 稳定错误码枚举

- **做什么**：`CapabilityErrorCode` enum + `CapabilityResult::failure_code()`。

---

## 8. P4 — 有界循环与预算（SEC-12）

### 4.1 连续重复工具调用检测 🔄

- **做什么**：同一工具 + 相同 arguments 连续重复达阈值 → `repeated_tool_call:<name>`；阈值进 `RuntimeConfig`；不同参数不误伤。

### 4.2 `max_steps` 按角色生效

- **现状**：`RuntimeConfig` / `with_max_steps` 存在但产品路径从不调用（恒 32 步）；`RoleSpec.max_steps` 不生效。

### 4.3 run 级 wall-time 预算

- **做什么**：run 级超时 → fail-closed，写 `run.budget_exceeded`。

---

## 9. P5 / P6 — 事件协议与 context 组装

（详细卡见 `reference-agent-audit/00-unified-agent-flow.md`；验收测试名见 §1 表。）

- **5.1** wire 加 `sequence`/`epoch`（additive，`PROTOCOL_SCHEMA` 不动）
- **5.2** 补齐 `Usage` / `ToolCall` / `ApprovalRequested` / `Error` 事件
- **5.3** terminal 必达 + replay cursor（`Last-Event-ID`）
- **5.4** 控制面单活跃 turn 守卫（现在只有 Web 有）
- **6.1** `PromptSection{name, order, text}` + `render_prompt()` + provenance
- **6.2** 预算覆盖 tool schemas 与 system prompt
- **6.3** `RoleSpec.prompt` 接到 provider 的 system message

---

## 10. P7 — 第二条执行循环（❓ 需要你拍板）

- **现状**：`kiana-entrypoints/src/runner.rs:633/1095` 直连 `execute_tool_calls_with_permission_handler`，绕过 broker 与 ControlPlane；`kiana-tools/src/agent.rs:2937` 生成的团队 agent 脚本会带 `--resident-teammate`，因此这条路径**可达**。
- **问题**：这违反仓库自己的硬边界（"Keep model/tool execution brokered"）。
- **两条路**：(a) 迁移到 `DaemonHost` 脊柱；(b) 明确标记 legacy 并冻结，同时确保它不进产品帮助文案。
- **产出**：一份书面决定 + 文档更新。**决定前不动代码。**

---

## 11. 追加区（你写这里，我接着派给 Codex）

在下面按模板加行即可；我会把它拆成任务、排进队列，做完回填 §1 与 §3。

**模板**

```text
- [ ] 标题：一句话说清要什么
      验收：怎么算做完（能写成测试名最好）
      边界：不许碰什么
```

**待办**

```text
（在这里追加）
```

---

## 12. 每步的完成定义（不许放宽）

一步算完成，必须同时满足：

1. 代码在 `master` 上，CI 的 `release-smoke` **全绿**；
2. 验收测试名出现在 §1 表里，且**先红后绿**（有观察记录）；
3. `CURRENT_STATUS.md` 有绑定源码快照的证据块；
4. 没有删除、放宽或跳过任何既有断言；
5. 冻结项没有被打开。

**不算完成**：代码存在但没进产品路径、单测绿但 CI 红、一次本机成功就写成 `durable`/`live`、把 `target` 改写成 `implemented`。
