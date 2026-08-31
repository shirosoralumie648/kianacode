# deepseek-harness — 源码审计

- **状态：** `audited`
- **参考路径：** `reference/deepseek-harness`
- **主要运行时：** deepseek-harness
- **审计范围：** `packages/core, packages/llm, packages/shell, packages/subprocess, packages/session, examples/headless-agent`

本报告基于源文件、测试、manifest 和 executable entrypoints。

## 端到端流程

- 1. Loader 启动 headless composition 并创建配置好的 root Agent/Session。
- 2. runFixtureTurn 等待 startup idle，创建 user message，附加 committed-event observer，并调用 Agent.followup。
- 3. Inbox wake 打开一个 turn；preStep 认领 message，组装 scoped prompt/context，并记录 turn/step/user events。
- 4. buildRequest 从 session history 推导 surface，渲染 system/tools，准备精确的 model route，记录 request metadata，并冻结 request。
- 5. LLM waterfall 到达 DeepSeek adapter；fetch 接收 SSE，translate 发出 protocol chunks，BlockAssembler 构建 assistant message。
- 6. tool-call block 被持久记录，tool policy/approval/guards 运行，Bash 解析 sandbox/workdir/env 并通过 shell→subprocess 执行。
- 7. Tool result 按 model order 被最终化/物化/记录；additional context 进入 next-step inbox；循环重复 model call。
- 8. stop/no-tool result 结束 step/turn；idle 被发布，fixture flush persistence，stdout 包含 event JSONL 加 final result。
- 9. 后续 resume 时，JSONL coordinator 加载并验证 contiguous prefix，必要时修复 torn tails，创建新的 Session，并从派生 messages 恢复 AgentLoop。

## 组件与边界

- 主要运行时：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core, packages/llm, packages/shell, packages/subprocess, packages/session, and examples/headless-agent` 中的 TypeScript ESM Cordis plugin composition。主要具体路径使用 direct DeepSeek fetch/SSE adapter 和 local Bash executor。辅助路径包括 llm-pi-ai、session-persistence-sqlite 和 PowerShell tool/provider packages。
- Ingress 和发布：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/examples/headless-agent/tests/fixtures/headless-driver.ts:14-31` 启动 Loader，调用 runFixtureTurn，将规范 session events 流式传输到 stdout，报告 result，并释放 context。`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/test-support/loader-smoke/src/agent-turn.ts:40-57` 恰好解析一个 root agent；`:59-83` 创建 user message 和 event observer；`:85-97` 调用 followup，等待 idle，flush persistence，并返回 output/usage。AgentLoop startup/factory configuration 位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/agent-loop/src/index.ts:295-381`。Agent creation/resume 和 rollback-covered publication 位于 `:453-577`、`:580-645` 和 `:653-710`。
- Prompt/context 和 session：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/agent-loop/src/agent.ts:224-243` 认领 inbox input，组装 system-prompt sections，投影 runtime context，并允许 pre-step waterfall rewrite/reject。Turn/step boundaries 和 durable user events 位于 `:245-329`。每个 request 从 `session.deriveMessages()` 推导 boundaryMessages，渲染 system/tools，解析 adapter defaults，在 `:426-515` 记录 request/header 和 request/context，并冻结带 sessionId 和 signal 的 marked request。`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/session/src/index.ts:569-655` 在提交和发布 session/event 前对 append candidates 进行 snapshot 和验证；`:670-747` 折叠 request metadata，并将有序 surface 投影为 LLM messages。
- Model request/stream：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/llm/llm/src/index.ts:816-869` 捕获 adapter generation、exact-model metadata、retry policy、defaults 和 one-shot prepared stream；`:893-971` 将 adapter construction/iteration failures 规范化为 terminal error/aborted chunks 并关闭 iterators；`:974-999` 进入 llm/stream waterfall。`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/llm/llm-deepseek/src/adapter.ts:421-431` 解析 model/connection generation；`:433-509` 绑定一个 connection/key，合并 cancellation，应用 idle watchdog，并排空 iterator；`:511-665` 序列化 request，POST `/chat/completions`，分类 HTTP/transport failures，重试 stale file IDs，并将 SSE 输入 translation。`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/llm/llm-deepseek/src/translate.ts:86-185` 累积 text/reasoning/tool-call fragments，映射 usage 和 finish reasons，仅在 [DONE] 时发出 block-end/usage/finish，并拒绝 malformed 或 truncated streams。
- Tool parsing、policy、execution 和 reinjection：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/agent-loop/src/agent.ts:331-419` 追加 assistant chunks/message，通过 agent/request-error 重试 request errors，提取 tool-call blocks，执行它们，并将 additional contexts 排入 next-step inbox。`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/agent-loop/src/tool-calls.ts:58-109` 解析 JSON arguments（将 invalid JSON 保留为 text），绑定发起的 Agent，并选择 execution mode；`:120-245` 实现 exclusive barriers、bounded parallel groups、ordered result commits、abort draining 和 synthetic skipped-call errors；`:247-287` 追加由 sourceEventSeqs 关联的 durable tool/call 和 tool/result events。`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/tools/src/index.ts:142-189` 声明 pre-execute、execute、post-execute 和 code-dispatch policy seams；`:1328-1507` snapshot arguments，处理 code-mode collapse/unknown tools，解析 approval 和 monotonic guards，并 fail-close 为 error results；`:1532-1645` 将 cancellation 融入 body，等待 started work，应用 post/final content policy，物化 frozen output，并通知 observers；`:1678-1729` 映射 missing approval、agentless calls、allowed-once、rejection、cancellation 和 unavailable outcomes。Bash 是位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/shell/tool-bash/src/index.ts:190-233` 和 `:330-390` 的 model-facing tool：解析 standing sandbox policy，在执行前请求 escalation approval，解析 workspace/env，支持 foreground/background execution，并将 aborts 转换为 structured tool errors。
- Termination、cancellation、errors 和 output：Agent turn control 在 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/agent-loop/src/agent.ts:112-139`（followup/steer/inject/cancel）、`:171-220`（wake/driver containment/idle）、`:294-329`（turn stopping、带 pending inbox 的 restart 和 turn end reasons）以及 `:301-314`（structured LLM/unknown error classification）中是明确的。Step 在 no tool calls、returns max-tokens 或 tool concludes 时完成；否则 tool contexts 在 `:409-418` 创建另一个 step。Local Bash timeouts 和 abort classification 位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/shell/bash-local/src/index.ts:211-239`。Process-tree cancellation、bounded output/spill files、SIGTERM-to-SIGKILL escalation 和 wait-for-tree-exit 位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/subprocess/subprocess-local/src/spawn.ts:326-542`。Session-event JSONL 是可观察的 UI/test stream：headless-driver.ts:20-25 将每个 event 和 final result 写成 JSONL；tool definitions 在 tool-bash/src/index.ts:323-328 提供 presentation/render functions。
- Persistence 和 recovery：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/session/src/index.ts:499-547` 验证 contiguous replay/fork seeds，冻结它们，并追加 end-seed marker；`:830-947` 在 collision 和 rollback protection 下准备/创建/进入 sessions。`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/session/session-persistence/src/coordinator.ts:127-214` 定义 backend contract，包括 atomic first materialization、contiguous append、crash repair 和 close-after-quiescence；JSONL stable-read/torn-tail handling 位于 `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/session/session-persistence-jsonl/src/index.ts:208-235` 和 `:292-344`。Resume 使用带 fused cancellation 的 persistence.prepare，随后在 agent-loop/src/index.ts:653-710 中 setup/publication。端到端 resume test 证明 fresh context 在第二个 prompt 前重新水合之前的 history，位于 examples/headless-agent/tests/resume.e2e.ts:32-66。
- 覆盖证据：`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/examples/headless-agent/tests/keyless-smoke.e2e.ts:14-47` 启动 real Loader，执行 real Bash round trip，验证 tool/result/output/usage，并检查 zstd persistence。full-loop.e2e.ts:27-48 是 key-gated real DeepSeek 加 real Bash。resume.e2e.ts:32-66 覆盖 dispose、fresh-context resume、history reconstruction 和 model recall。fixtures/cli-mock-llm.ts:13-69 提供确定性的 tool-call→tool-result→final-answer behavior 和 request-step configuration。Package-level agent-loop tests 覆盖 cancellation、resume、request reconstruction、tool calls、ordering 和 errors；shell 和 subprocess tests 覆盖 executor behavior、process exit、inspection 和 cancellation。

## 状态、持久化与恢复

- 生命周期所有权是明确的：factory 跟踪 startup/live agents；publication 将其加入 session 和 agent registries；disposal 取消、等待 idle、释放 scoped context，并按逆序分离 registries。
- Session logs 是 append-only、deep-frozen、contiguous、surface-validated 的，并暴露 immutable snapshots；derived messages 会被缓存，并在 surface replacement 时重建。
- Prepared LLM calls 固定 adapter generation 且只允许一次 dispatch；adapter failures 是由 loop 的 request-error policy 消费的 terminal stream chunks。
- Tool results 在 post-execute/finalization 后提交，并与其 call event 关联；additional contexts 在 next-step boundary 排队。
- Persistence recovery 区分 unsupported format/corruption/torn tails，并在 publication 前使用 stable file revisions。

## 工具、策略与副作用

- find、grep、nl、Read；仅源码检查；未修改文件；审计没有使用 README/CLAUDE/AGENTS/USER/docs/changelog/git-history 内容。

## 输出与呈现

- 总体审计：源码实现了从 ingress 到 model streaming、tool policy/execution、result reinjection、termination、event output 和 durable resume 的完整事件溯源 agent turn。
- 最强的架构特性是单一 durable Session surface：request history 从已提交事件而非临时 loop arrays 派生，并且 request/header/context events 使 model-call metadata 可重放。
- Cancellation 贯穿 agent、LLM、tool、shell 和 process-tree layers；started tools 被排空，unstarted model calls 接收 synthetic durable errors，从而保持 replay validity。
- Approval 在 ToolRuntime seam 处 fail-closed，Bash sandbox escalation 在扩大 execution policy 前增加第二条明确的 approval path。
- 测试栈包括 real Loader composition keyless smoke、deterministic model/tool round trip、real-provider loop 和 cross-context persistence resume。

## 测试与验证

- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/examples/headless-agent/tests/keyless-smoke.e2e.ts`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/examples/headless-agent/tests/full-loop.e2e.ts`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/examples/headless-agent/tests/resume.e2e.ts`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/examples/headless-agent/tests/fixtures/cli-mock-llm.ts`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/agent-loop/tests/cancel.spec.ts`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/agent-loop/tests/resume.spec.ts`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/core/agent-loop/tests/tool-calls.spec.ts`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/llm/llm-deepseek/tests/sse.spec.ts`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/session/session-persistence-jsonl/tests/jsonl.spec.ts`
- `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/reference/deepseek-harness/packages/subprocess/subprocess-local/tests/spawn.spec.ts`

## 优势

- 完整、可通过源码追踪的主要路径，具有明确的逐行生命周期和数据转换。
- 持久化事件溯源统一了 prompt reconstruction、replay、UI event output 和 resume。
- 在异步 model、tool、shell 和 process operations 之间具有强 cancellation containment。
- 具有 monotonic guard semantics 的 fail-closed policy 和 approval handling。
- Real-composition keyless smoke 加 deterministic mock、real-provider 和 cross-context resume coverage。

## 风险与缺口

- Persistence 是 write-behind：Session.append 在 backend durability 之前提交到内存并通知 session/event。headless fixture 明确调用 sessions.flush，但未执行等效 flush 就退出的 host 依赖 lifecycle teardown 排空 writes；这在 core/session/src/index.ts:569-603 和 loader-smoke/src/agent-turn.ts:85-97 可见。
- Model request 从 durable surface 组装，但 system-prompt/runtime context changes 被表示为 plugin-owned user messages；正确性取决于这些 snapshot/replacement events 被 Session surface 接受。RuntimeContextProjection 在 agent-loop/src/runtime-context.ts:23-73 实现了这一点。
- Approval 按 composition 是可选的。ToolRuntime 在没有挂载 approval service 时有意将 ask 降级为 denial（core/tools/src/index.ts:1678-1704），因此期待交互式审批的 composition 必须明确挂载并路由该 service。
- DeepSeek image/file handling 在 llm-deepseek/src/adapter.ts:537-599 引入了额外的 retry 和 fallback state machine；这超出了简单 text/Bash path，在启用 image-capable models 时应单独测试。
- Keyless smoke 验证 assembled Loader path 和 zstd artifact，而 real DeepSeek path 在 full-loop.e2e.ts:27-48 中受环境控制；因此 provider-side stream/HTTP behavior 主要由 adapter/SSE tests 单独覆盖，而非默认 keyless run。
- Tool concurrency 有界并按 model order，但 parallel tool bodies 可能重叠；要求严格串行副作用的部署必须适当配置 ToolRuntime/AgentLoop parallel cap 或 tool execution mode（agent-loop/tool-calls.ts:81-99、:197-229）。

## 映射到 Kiana

- Primary Agent implementation 映射到 TypeScript Cordis AgentLoop/ReactLoopAgent runtime，而不是独立的 Python 或 native agent runtime。
- Ingress 是仅用于测试的 headless Loader driver；durable identity 是由 AgentRegistry 和 Session 共享的一个 SessionId。
- Model-facing state 可从 Session surface events 重建；transport 在主要路径中使用 direct DeepSeek SSE adapter。
- Tool authority 在 ToolRuntime policy/approval 与 Bash/Shell/Subprocess providers 之间分离；persistence 是独立的 SessionPersistence capability。

## 源码证据

- Concrete keyless path 是确定性的：cli-mock-llm.ts:35-43 在不存在 tool result 时发出 bash tool call，随后 :46-56 在 result 被注入后发出 final text。
- Loop 通过 Session append API 写入 turn/start、step/start、user/message、assistant/chunk、assistant/message、tool/call、tool/result、step/end 和 turn/end；每次 model call 前添加 request/header 和 request/context。
- Fixture observer 在 log push 后接收 committed session/event notifications，而 final driver 等待 idle，并在返回前明确调用 sessions.flush。
- 当 loop 使用 session id 标记 request 时，DeepSeek adapter 发送 x-deepseek-harness-session-id（llm-deepseek/src/adapter.ts:520-528），将 provider traffic 关联到 durable session。
- Resume test 检查 resumed.session.deriveMessages() 在发出第二个 prompt 前包含 prior secret（examples/headless-agent/tests/resume.e2e.ts:49-65）。
