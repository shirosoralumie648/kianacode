# 流式（token streaming）解冻方案

给产品主人拍板用。先说结论，再给证据和选项。

---

## 0. 一句话结论

**代码的“管子”基本都在，但产品主路径把阀门关死了，所以今天不算支持流式。** 推荐先走方案 A（最小切片：只做接缝 + 单个界面 opt-in、默认关闭），等真实 provider 和证据条件具备再谈端到端。**第一步不是写代码，是先签一份书面解冻决定**——因为 FZ-LIVE 写明“重开需书面决定”。

---

## 1. 现状：代码在哪，为什么现在不算支持

一句话：**服务层已经能把 Anthropic 的 SSE 流解析成事件，但产品主路径的模型客户端写死 `stream: Some(false)`，而且全仓没有一处产品代码调用流式接口——唯一的调用点在测试里（`#[cfg(test)]`）。**

关键文件：

| 位置 | 事实 |
|---|---|
| `kiana-services/src/api/streaming.rs:11-95` | `StreamEvent` 完整：文本 / thinking / 工具调用增量、usage、stop_reason、Error 都有 |
| `kiana-services/src/api/streaming.rs:97-151` | `AnthropicClient::stream_message` 真的发 `stream=true` 并解析 SSE |
| `kiana-services/src/api/provider.rs:206-220` | `Provider` trait 有 `stream_message`；Anthropic 是 `Native`，OpenAI-compatible / Ollama / fake 是 `Synthetic`（先非流式请求，再事后合成事件） |
| `kiana-daemon/src/model_client.rs:171-207` | 产品路径 `ProviderModelClient::complete` 固定 `stream: Some(false)`，只调 `create_message` |
| `kiana-runner/src/model.rs:177-186` | `ModelClient` trait 只有 `complete()`，没有流式方法 |
| `kiana-runner/src/harness.rs:342-396` | 模型整段返回后，才 push 一条 `RunnerEvent::Delta{text:整段}` |
| `kiana-entrypoints/src/runner.rs:1178-1191` | 唯一的产品侧 `stream_message` 调用点，但整个函数是 `#[cfg(test)]` |
| `kiana-entrypoints/src/sdk.rs:1128-1143` | 旧 SDK 的“流式”是把整段文本包成**一个** `ContentBlockDelta` 发出去——伪流式 |
| `kiana-entrypoints/src/web.rs:414,463,1121` | Web 的 `health`/`state` 硬编码 `"streaming": false`，并有断言 |
| `docs/coding-pack-matrix.md:168` | FZ-LIVE：`live provider 接入 / token streaming 当完成`，冻结到有真实 provider adapter、对账和证据块；重开需书面决定 |
| `AGENTS.md:166-167` | §7 明确“不许打开 token streaming” |

结论：**“有代码”不等于“已接入”，更不等于“已证明”。** 现在既不是 live，也不是 unsupported_streaming，只是没接。

---

## 2. 要解决什么：拆成四层看

“接流式”不是一个开关，而是四层东西。每一层现状和差距如下。

### 2.1 服务层（provider → 模型客户端）

**现状**：`StreamEvent`、Anthropic SSE 解析、`Provider::stream_message` 都齐了；错误分类也有（`ApiError::classify`）。
**差距**：
- 只有 Anthropic 是**真流式**；OpenAI-compatible / Ollama / fake 的 `stream_message` 是**先发非流式请求、再把完整响应拆成事件**（Synthetic），线路上从不发 `stream=true`。
- 产品路径 `ProviderModelClient::complete` 写死 `stream: Some(false)`，且 `ModelClient` trait 没有流式方法——**接缝根本不存在**。
- 流式路径**没有重试**：现有 `with_retry` 只包 `create_message`；`StreamEvent::Error` 和 SSE 解析错误都不可重试。

### 2.2 运行时与协议（runner → core → wire）

**现状**：`RunnerEvent::Delta` 已存在，`ControlPlane::drive_run` 把它投影成 `run.delta` 写进事件账本。
**差距**：
- `Delta` 的粒度是**每个模型轮次一条整段文本**，不是逐 token（`harness.rs:390-396`）。
- `run.delta` 目前是 **write-only**：receipt 投影、UI、client 都不读（全仓只有 `kiana-core/src/lifecycle.rs:489` 一处写它）。
- 事件账本是**拉取式**：`EventStorePort` 只有 `append/read_*`，没有 subscribe/broadcast，实时看只能轮询。
- wire 协议 `kiana.protocol.v1` 是严格单发单收：`ResponseEnvelope` 只有一个 `output`，没有任何增量字段或订阅通道；daemon 对 `schema` 做**精确字符串比对**，不能随手改版本串。

### 2.3 界面（CLI / TTY workbench / Web + Electron）

**现状**：三个界面都是“run 结束拿一次完整 `ResponseEnvelope` 再渲染”。
- CLI `kiana run` 完成后一次性 `println`；`-p --include-partial-messages` 走的是 SDK 的伪流式。
- TTY workbench 用 ratatui，每 80ms 重绘，但消息在收到完整响应后才 push 成不可变 `ChatMessage`。
- Web 用 `fetch` 轮询 `/api/state`（仅运行中开 1s 定时器），`/api/run` 是阻塞 POST；**没有 SSE / WebSocket / EventSource**，health 硬编码 `streaming:false`。
- Electron desktop 只是把 web URL 装进 `BrowserWindow`，Web 改了它自动继承。

**差距**：根因不在界面，在脊柱——没有增量通道，界面就无从消费。

### 2.4 证据与状态（冻结纪律）

**现状**：token streaming 在账本里是 `not_supported` / `source`（最低状态 + 最低证明等级）；`docs/coding-pack-matrix.md:168` 的 FZ-LIVE 把它和 live provider 绑在一起冻结。
**差距**：
- 解冻是**治理动作**，不是“把 streaming.rs 搬过去”：先书面决定 → 拆 FZ-LIVE 措辞 → 按 AGENTS §8 先补负向证据 → 首发只能落 `partial` + `local_behavior`（cassette/fake）。
- 至少 17 份文档 + 5 处代码要同步改，口径必须一致。
- 触碰多条 SEC-P0：SEC-05（secret 不泄漏）、SEC-08（取消不得返回 completed）、SEC-09（无法确认→result_unknown）、SEC-11（流式输出不授予权限）、SEC-12（配额与背压）。

---

## 3. 候选方案

三个选项。每个都写清“做什么 / 不做什么 / 工作量 / 风险 / 验收”。

### 方案 A：最小切片（接缝 + 单界面 opt-in，默认关闭）——推荐

**做什么**
1. 给 `ModelClient` 加一个**带默认实现**的流式方法（如 `complete_streaming`，默认调用 `complete()` 后把整段 text 作为一条 delta 喂出去）。这样 `Arc<dyn ModelClient>`、`ScriptedModel`、`UnavailableModel`、测试模型全部零改动即兼容。
2. `ProviderModelClient` 在**显式开关开启且 provider 是 `native_streaming`** 时走 `provider.stream_message`，按 index 聚合文本 / `InputJsonDelta` / usage / stop_reason。
3. 只在 **CLI** 加一个 opt-in（例如 `--stream`），订阅增量并逐块 `write + flush`；工具事件、文件变更仍按最终回执打印。
4. Web 的 `streaming` 保持 `false`；默认行为（开关关闭）与今天逐字节一致。

**不做什么**
- 不做 Web / desktop 的 SSE。
- 不做 OpenAI-compatible / Ollama 的原生 SSE 解析（保持 Synthetic 或整段回退）。
- 不改协议版本，不加 required 字段。
- 不解 live provider。

**工作量**：M（约 1 个接缝 + 1 个聚合 + CLI 渲染 + 测试）。
**风险**：中低。主要坑是 async trait 默认方法的对象安全、工具调用 input 的 JSON 聚合要对齐测试版逻辑、流式路径的重试策略要单独定义。
**验收方式**：默认关闭时 golden / cassette 行为零回归；开启后用 fake/synthetic 分块证明逐块到达 CLI，且最终文本与整段一致；负向路径见第 6 节。

### 方案 B：端到端全接（provider → runner → 协议 → 三界面）

**做什么**
1. A 的全部内容，但 provider 层同时补 OpenAI-compatible / Ollama 的原生 SSE 解析。
2. 给 runner / harness 加**运行中事件通道**（broadcast 或 mpsc），让 delta 在产生时即可被订阅，而不是等 `send()` 返回整批。
3. 协议层加 additive 的订阅能力（保持 `PROTOCOL_SCHEMA=v1` 不变，用 `serde(default)` 可选字段或独立订阅请求）。
4. Web 加 `GET /api/events`（axum `Sse`），前端用 `EventSource` 增量追加；desktop 自动继承。
5. 补 in-flight 取消、跨 chunk 脱敏、断线重连去重。

**不做什么**：不绕过 `DaemonHost → ControlPlane → KianaHarness`；不接旧 SDK 的第二执行循环；不把 `run.delta` 改成逐 token 落账。
**工作量**：L（跨 5+ crate，含协议、core、三个界面、证据套件）。
**风险**：高。动的是授权/事件事实源的执行脊柱；取消、审批卡点、CAS 写入、协议兼容、SEC-12 配额都要同时处理；且按 FZ-LIVE，真流式要宣称完成还需真实 provider adapter + 对账 + 证据块。
**验收方式**：第 6 节全清单 + 三界面端到端 + 老客户端兼容 + 负向证据块。

### 方案 C：维持冻结，只把事实写进文档

**做什么**：不改代码，只在文档里如实写明“`streaming.rs` + `Provider::stream_message` 已存在，但产品主路径 `stream: Some(false)`、无调用方、SDK 是伪流式”，并保留 FZ-LIVE 冻结。
**不做什么**：不接任何流式。
**工作量**：S（几处文档）。
**风险**：低。但产品体验不变，用户仍看不到流式；且没有为将来解冻铺路。
**验收方式**：文档口径与代码一致，`streaming:false` 与账本一致。

---

## 4. 推荐：方案 A，第一步是“先签字，再接缝”

**为什么推荐 A**
- 它把真正的技术缺口（`ModelClient` 接缝）补上，同时**不碰**最贵、最危险的部分（协议版本、SSE 端点、执行脊柱）。
- 默认关闭，release gate、cassette、现有界面行为零风险。
- 用 fake/synthetic 就能验证，符合 FZ-LIVE “首发只到 `local_behavior`”的纪律，不夸大成 live。
- 一旦真实 provider 条件具备，B 只是在这个接缝上加管道，不是从零开始。

**为什么不直接上 B**：B 的成本和风险高一个量级，而且它的“完成”依赖 live 证据，现在拿不到；在没有书面解冻的前提下动协议和脊柱，会踩 AGENTS §7/§10 和矩阵 §5“未改表就标完成”的先例。

**第一步具体做什么（顺序不能反）**
1. **书面解冻决定**（硬门槛）：写清解冻范围（只 token streaming、只本地 CLI opt-in、默认关闭）、首发证明上限（`local_behavior`，只许 Fake/Synthetic）、明确 live provider 仍冻结、不碰 HTTP MCP。落到 `docs/coding-pack-matrix.md` FZ-LIVE 行 + `AGENTS.md §7`，作为后续可引用的记录。
2. **第一个代码 PR = 接缝**：给 `ModelClient` 加带默认实现的 `complete_streaming` + 单测（默认退化为一条 delta）。此时**产品行为零变化**，可以独立 review 和回滚。
3. 之后才是 `ProviderModelClient` 聚合 + CLI opt-in 渲染 + 负向测试。

---

## 5. 风险与冲突（逐条）

| 冲突点 | 具体风险 | 处理原则 |
|---|---|---|
| **cassette / 脚本测试** | `ScriptedModel::complete` 从队列弹一个完整 `ModelOutput`，没有分块概念；`KIANA_HARNESS_SCRIPT` 优先级高于 provider。若给脚本加“切成 N 块”会动 cassette schema。 | 用**带默认实现的接缝**：默认退化为 `complete()`→单条 delta，cassette 零改动。要测分块另加形状，别改旧 JSON 语义。 |
| **compaction** | `compact_if_needed` 在模型调用前跑；历史用最终 `ModelOutput` 一次性拼。 | 守住分层：**delta 只用于展示，历史仍用最终整段拼一次**。compaction 和事件投影都不用动。 |
| **redaction** | 现有脱敏是 marker 匹配（`token=`/`api_key=`/`bearer`）且只在单条字符串内有效；逐 token 时 secret 可能被拆到相邻 chunk，匹配失败就泄漏。 | 账本**只存聚合后已脱敏文本**；实时通道若直出 UI，需明确是否也在通道侧脱敏，并测跨块拆分场景。 |
| **取消** | 现在没有 in-flight 模型取消：`cancel` 只 `take_run` 返回 `Failed{cancelled}`，打断不了正在进行的模型调用。真流式后用户会期待“流到一半就停”。 | 若做真流式，流循环必须检查 `cancel_rx`；按 SEC-08，**无法确认停止时不得返回 completed**，终态应 `cancelled` 或 `result_unknown`。A 阶段可先不承诺中途取消，但文档要写清。 |
| **审批** | harness 在遇到 tool_calls 前已把文本 delta 发出，“先出文本、再弹审批”内部成立；但客户端拿到的是一次性 `ResponseEnvelope`，审批前看不到已流出的文本。 | 流式要真正可见，必须有**独立于 envelope 的通道**；半截文本与审批卡点的一致性要定义（保留 / 丢弃 / 标注未完成）。 |
| **事件账本** | `run.delta` 现在 write-only；改成 per-token 会让每次 append 都 `read_stream` 算版本 + idempotency + CAS，写放大、日志膨胀、同 run CAS 争用，还可能击穿 SEC-12 配额。 | 账本**保持粗粒度**（按轮次或固定 flush 间隔聚合）；细粒度 delta 走**易失通道**，且绝不能变成 AGENTS §6 禁止的“第二事实源”。 |
| **协议兼容** | daemon 对 `schema` 精确比对，改版本串会被老客户端硬拒；给 `RequestBody` 加 required 变体，老 daemon 解新请求会 tag 反序列化失败。 | 优先 **additive 可选字段**（`#[serde(default)]`）或独立订阅请求，`PROTOCOL_SCHEMA` 不动；老客户端只读 `output`，多余字段/新 event kind 会被忽略。 |

**关于网页传输与 HTTP MCP 冻结的区别（必须说清）**：如果将来做 Web SSE，它是 **loopback 本地 UI 面**——同进程、走 `x-kiana-web-token` + Host/Origin 校验、只投影 `DaemonHost` 的运行状态，服务端到浏览器单向推送 `run.delta`，**不暴露模型可见工具、不走 MCP JSON-RPC**。这与 `kiana-entrypoints/src/mcp.rs` 的 `/mcp`、`/sse`、`/message`（MCP transport，FZ-MCP-HTTP 冻结，当前返回 `mcp_transport_unsupported`）是两回事。本方案（A）不新增 Web SSE；若将来上 B，必须沿用这个区分，且不得借“流式”之名打开 HTTP MCP。

---

## 6. 验收清单（怎么算完成）

按仓库习惯，先证明拒绝路径，再证明成功路径。以下清单对应方案 A；B 需要在此基础上加协议与三界面项。

**正向**
- [ ] 开关**关闭**时，行为与今天逐字节一致：`bash scripts/harness-golden-smoke.sh`、cassette 回归、`cargo test --workspace` 全绿。
- [ ] 开关**开启**、用 Fake/Synthetic provider 时，分块文本逐块到达 CLI，最终文本与一次性整段返回完全一致。
- [ ] 工具调用流式：`InputJsonDelta` 拼装出的 `tool_use.input` 与非流式结果逐字段一致，工具仍经 broker + policy + approval，模型可见工具清单**零新增**。
- [ ] usage / stop_reason 在流式路径正确聚合（注意 `MessageStart` 给 input_tokens、`MessageDelta` 给 output_tokens）。

**负向（缺一不可）**
- [x] **流到一半取消**：终态为 `cancelled` 或 `result_unknown`，绝不写 `completed`，取消后不再发 delta。（P1-03 证据块 + `daemon_host::cancelling_mid_stream_never_completes_or_emits_a_late_delta`）
- [x] **流式内容脱敏**：sentinel secret（Bearer/Basic/token/API-key/X-Api-Key）不落账本、不出现在 stdout/stderr/argv/env，且覆盖"secret 被拆到相邻 chunk"的场景。（`daemon_host::split_secret_across_stream_deltas_...` + `kiana-domain` streaming redactor 跨块单测）
- [ ] **断线 / 重连**：连接中断不产生重复 item，未收到 terminal event 前界面不得显示"完成"；连接失败 fail-closed。
- [x] **老客户端兼容**：老 daemon 解新请求不失败；老客户端忽略新增字段 / 新 event kind；`PROTOCOL_SCHEMA` 不变。（`kiana-protocol::run_stream_events_are_additive_and_unknown_events_are_ignored`）
- [x] **provider 不支持 streaming**：返回机器可读的 `unsupported_streaming`，fail-closed；若选择自动回退，必须显式声明并测到，不能静默装等价。（`streaming_on_fails_closed_for_non_native_provider`；流在终止事件前结束另返回 `provider_stream_incomplete`，见 2026-09-09 证据块）
- [x] **事件账本**：确认没有逐 token 写 `RuntimeEvent`；CAS 无争用回归；`run.delta` 仍是展示投影，事实源仍是 EventLog + receipt。（2026-09-09 账本粒度证据块：按轮次聚合，不再逐块落账）
- [ ] **单一脊柱**：全部经 `DaemonHost → ControlPlane → KianaHarness`；未接旧 SDK 第二执行循环（`sdk.rs` / parked `tui.rs`）。

---

## 7. 需要主人拍板的问题（5 个）

1. **要哪种“流式”？** (a) 真 provider token 流（要重开 FZ-LIVE）；(b) step 级 delta（每轮一次，不碰 provider 流式）；(c) UI 事后合成回放（今天 SDK 的做法，不算流式）。“完成”按哪种定义？
2. **解冻范围与默认？** 只本地 CLI opt-in 且默认关闭，还是也含 workbench / web？live provider 是这次一起解，还是继续冻结？
3. **首发证明上限？** 只到 `local_behavior`（Fake/Synthetic）即可，还是必须一步到位满足 FZ-LIVE 的 live（真实 provider adapter + 对账 + 证据块）？
4. **协议落点？** additive 可选字段（保持 `v1`）、独立订阅请求，还是 Web SSE 端点？老客户端兼容口径按哪个来？
5. **中断语义？** 流到一半取消/失败时，已流出的文本保留还是丢弃？终态写 `cancelled` 还是 `result_unknown`？重试是“只允许首 token 前”还是允许流中断后重试？

---

## 8. 决策记录（2026-09-09，产品主人拍板）

| 问题 | 决定 |
|---|---|
| 要哪种流式 | **真 provider token 流**（据此重新打开 FZ-LIVE） |
| 解冻范围 | **三个界面都要**：命令行、文件夹工作台、网页版（loopback SSE） |
| 首发证明上限 | **必须到 `live`**：真实 provider adapter + 对账 + 证据块；不接受只用 Fake/Synthetic 就宣称完成 |
| 中断语义 | **保留已流出文本并标注「已中断」**；终态 `cancelled`；副作用无法确认时 `result_unknown` |
| 开关默认 | **`auto`**：`KIANA_STREAMING=off|auto|on`，默认 `auto`——provider 支持就流，不支持就退回整段，但**退回必须显式记一条事件**，不许静默装成等价 |
| 网页传输 | **SSE**（单向推送、自带断线重连，代码最少）；取消/审批仍走现有 HTTP 接口。WebSocket 依赖已就位（`axum` 的 `ws` 特性 + `tokio-tungstenite`），留作将来需要双向通道时再用 |

据此按「方案 B 全量版」执行。开发期间用 opt-in 开关（默认关闭）保证既有行为零回归；`live` 证据块落地后，命令行与网页默认开启、保留关闭开关。

**仍然冻结、本次不碰**：HTTP MCP、支付/出行/打车/订票/IoT、企业租户与远程执行、自由多 agent 消息总线；`live provider` 只解冻「流式」这一条链路，其余 provider 能力（如批处理、文件接口）继续冻结。

## 9. 实施切片（每片独立可验证，顺序不可颠倒）

1. **书面解冻决定**：本文 §8 + `coding-pack-matrix.md` FZ-LIVE 行 + `AGENTS.md` §7。
2. **`ModelClient` 流式接缝**：带默认实现的 `complete_streaming`（默认退化为一条 delta）+ 单测；**产品行为零变化**。
3. **`ProviderModelClient` 原生 SSE 聚合**：Anthropic 先行，OpenAI-compatible / Ollama 随后；含工具调用 `InputJsonDelta` 聚合与 usage / stop_reason。
4. **运行时事件通道**：runner/harness 产生增量即可被订阅；**账本保持粗粒度**，不逐 token 落事件。
5. **协议 additive**：新增增量事件或可选字段，`PROTOCOL_SCHEMA` 不变，老客户端只读 `output` 不受影响。
6. **命令行 `--stream` 渲染**。
7. **文件夹工作台增量渲染**（ratatui）。
8. **网页 loopback SSE**（与 HTTP MCP 无关，见 §5 末尾的区分说明）。
9. **负向测试**：跨块脱敏、流中取消、断线重连、配额、老客户端兼容、不支持流式的 provider fail-closed。
10. **live 冒烟 + 对账 + 证据块**：真实 provider 跑通并留下回执后，`CURRENT_STATUS.md` 从 `not_supported` 翻到相应状态。

## 附：本方案不碰的红线

- 不新增模型可见工具。
- 不打开 HTTP MCP、支付/出行/IoT、企业租户、自由消息总线。
- 不绕过 `DaemonHost → ControlPlane → KianaHarness`，不新增第二条执行循环。
- 不把 Fake/Synthetic 说成 live，不把“代码存在”说成“已证明”。
