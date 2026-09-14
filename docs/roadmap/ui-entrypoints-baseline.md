# UI-00 入口基线与验收矩阵

> 快照日期：2026-09-14。本文是 `UI-00` 的 source-only 基线，不修改 feature status。
> 调用图与测试绑定由 12-agent survey 生成；运行时回执由 GitHub CI 负责。

## 1. 快照

| 项目 | 记录 |
|---|---|
| source snapshot | `05c3b288198d53e0363d84a30acb8cca9e56edad`（EXT-00 收口提交） |
| worktree 基线 | `master`，工作树干净 |

| 入口 | 文件 | SHA-256 |
|---|---|---|
| CLI 路由 | `kiana-entrypoints/src/cli.rs` | `bb7ee58554f9f1f7d45c2312f949d05ec25389afef08561a3ebd51874f54997f` |
| Web workbench | `kiana-entrypoints/src/web.rs` | `29ca4e963b70c23c65d18c6d8d1944a530b4b2af695d436ac4c1b9723649ba51` |
| 文件夹 workbench | `kiana-entrypoints/src/workbench.rs` | `f2272a99a1c4045bf9a9dc666b518428774e0b7e59d81c8e099a2dee0c8a0fbe` |
| workbench 聊天 | `kiana-entrypoints/src/workbench_chat.rs` | `ab742ce7583f510271f98c0a7958590a89575546d84d458b97d7f00167b6c4f5` |
| envelope 构造 | `kiana-entrypoints/src/harness_run.rs` | `a04d341aa4eee20ed42b4c7de424c8f9696b287ccc3308ee032c9c0d90be3e1e` |
| wire 协议 | `kiana-protocol/src/lib.rs` | `19b06c1e828bef31cf6fdbd52283e0bf2ed54b69ca4d1c6fed94dc385e376ace` |
| 客户端 | `kiana-client/src/lib.rs` | `106d37c5b520d36604b017550e4c9489012135a0e15ba80276279c9b97ed0e62` |
| 流总线 | `kiana-daemon/src/run_stream.rs` | `750bb78e46e5f1c42d3a773d0afcf468cc6713032ef47e30ffa100f3313db5c6` |

## 2. 四入口 → DaemonHost 调用图

| 入口 | 启动 | run | approval | cancel | resume | receipt | export |
|---|---|---|---|---|---|---|---|
| **CLI** `kiana run` | cli.rs:91→run_main:346（手写参数解析，无 clap） | `run_envelope`（harness_run.rs:65，stream 默认开） | `-p` 模式经 permission-prompt-tool（harness_run.rs:150-209）；`kiana approvals`/`approval <id>` 走 product_command.rs:76-106 | `--cancel`→`cancel_envelope`:420 | 仅 `kiana resume --session-id`（裸 `resume` 落 unknown command） | `--receipt`→:452 | cli_export 测试面 |
| **Web** `kiana web` | DEFAULT_BIND `127.0.0.1:3080`，非 loopback 拒绝 | `/api/run`（web.rs:466-479 路由表） | `/api/approvals` GET+POST decide:1186（重读服务端 profile） | `/api/cancel`:992 | `/api/resume`:1217 | `/api/receipt`:1079 | 历史会话列表（persisted_events 投影） |
| **Workbench** `kiana` / `--workdir` | workbench.rs:76-86 识别；默认 workspace-write 但 trust/control-plane 检查仍生效 | 聊天循环→DaemonHost | `/approvals` → `/approve <id>` / `/deny <id>`（workbench_chat.rs:668-736，重验证 available_decisions） | `/cancel`:753 | `/resume`:737 | `/receipt`:657 | 同 web 会话投影 |
| **Desktop** `contrib/desktop` | Electron 主进程 spawn `kiana web` 子进程（stdio pipe、detached 非 win32） | 经 web 面 | 经 web 面 | 经 web 面 | 经 web 面 | 经 web 面 | —（3 个 node --test：deb/desktop/workspace） |

共同底线：所有入口最终经 `LocalDaemonTransport → DaemonHost::handle → ControlPlane`，无第二执行循环；`tui` 停在 legacy SDK 流（`sdk::unstable_v2_prompt`），非产品路径。

## 3. 拒绝需求覆盖矩阵（7 项对抗复核）

| 拒绝需求 | 状态 | 证据 / RED 测试名 |
|---|---|---|
| 未授权 workspace 写拒绝 | ✅ 已覆盖 | `untrusted_workspace_write_does_not_create_file`（cli_run.rs:439，spawn 真实二进制）+ daemon `untrusted_run_never_reaches_model_or_broker` |
| Web Host/Origin 不匹配拒绝 | ✅ 已覆盖 | `web_rejects_wrong_origin_and_host_without_mutating_trust`（cli_web.rs:647，exact-listener） |
| 未知命令拒绝 | ✅ 已覆盖 | `unknown_and_untrusted_commands_are_blocked`（control_plane.rs:82，`command_unregistered`）+ REPL `unknown_command` 路由测试 |
| 过期 cursor/revision 拒绝或安全重放 | 🔴 RED | 实现存在（`subscribe_after` gap 计算 run_stream.rs:155-186、`advance_cursor` 三类拒绝 protocol lib.rs:605-626、`stream_cursor_invalid` web.rs:890-916），**零测试**。RED：`web_sse_stale_last_event_id_cursor_emits_stream_gap_snapshot_required` + `advance_cursor` 单测（stale-epoch/sequence-gap/duplicate） |
| 非 owner 动作拒绝 | 🔴 RED | 实现存在（`resolve_mutable_session` 拒 foreign session；continue 有 `wire_explicit_run_id_cannot_cross_session_owner`），cancel 无对应测试。RED：`web_cancel_from_foreign_session_is_rejected_without_mutation` + `wire_cancel_from_other_session_is_rejected` |
| 响应丢失 surfaced as result_unknown | 🔴 RED | UI 层只有 arrived/converged 两类 result_unknown 测试；丢失场景的分支是 `stream_closed_before_terminal`（web.rs:833、workbench_chat.rs:870，未测）与 `stream_terminal_missing:closed`（stream_render.rs:77-79，仅 Lagged 有测试）。RED：`lost_run_response_surfaces_result_unknown_instead_of_silent_failure` |
| 旧 epoch 请求重启后拒绝 | 🔴 RED | 实现存在（每 incarnation 随机 epoch run_stream.rs:51-62；`claim_ui_action` → `ui_action_stale` → HTTP 409 web.rs:862-888/230-232），**零测试**。RED：`claim_ui_action_rejects_an_old_epoch_after_restart`（含 e2e 变体：restart 后带 stale `x-kiana-ui-epoch` 头 → 409） |

## 4. 覆盖分类与既有测试绑定

- **deny**：cli_run（untrusted write、cancel_unknown_session）、cli_web（wrong origin/host、foreign bearer/session）、control_plane（unknown command、context bounds）——绑定到 §3 前三行。
- **happy**：cli_help/cli_print/cli_completion/cli_session/cli_architecture、workbench 聊天 roundtrip、web 会话列表（`fresh_web_app_lists_previous_sessions_from_the_event_ledger`）。
- **reconnect**：`web_sse_reconnect_emits_stream_gap_without_replaying_delta_items`（cli_web.rs:361，但重连不带 cursor，只断 `subscription_attached_after_run_started`——真正的 cursor 重连语义属 §3 RED 行）。
- **recovery**：`disk_receipts_survive_restart_and_do_not_overwrite_the_first_run`（daemon_host.rs:1854）、core 层 `receipt_without_a_terminal_event_is_result_unknown` / `missing_runner_terminal_event_is_result_unknown_and_replays_as_unknown`——core 收敛语义已测，UI 层丢失呈现未测。
- **physical**：desktop 三测试（Electron spawn、deb 打包、workspace 选择）由 `node --test` 跑，CI 在 release-smoke 尾部。

**baseline failure 记录**：无已知红的入口测试；4 项 RED 是覆盖缺口而非实现缺口（实现均已存在、测试未写）。`0 tests` 不记为通过——RED 测试名已列 §3，归属 UI-01+ 落地。

## 5. 并行 agent 交接说明

- UI-01（versioned UI protocol DTO）以本表 §2 路由清单为范围基线。
- 4 项 RED 测试是 UI-04（动作 CAS/idempotency/响应丢失）与 UI-05+（重连/恢复）的直接工单；全部有实现锚点，写测试即绿。
- `tui` 维持 parked 判定（legacy SDK 流），UI-* 不给它排期。
