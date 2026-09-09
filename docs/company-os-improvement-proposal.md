# Kiana 改进建议（参考项目驱动，待批准）

> 这份文档是「建议层」，不是规范：它汇总了 12 组只读分析对 `reference/` 项目的观察，整理成一份给你拍板用的清单。每条都写清楚参考了谁、Kiana 现在什么样、建议改什么、该动哪份文档；你可以逐条批「做 / 不做 / 以后再说」。真正的规范仍在 `docs/company-os-*.md`，当前事实仍以 `CURRENT_STATUS.md` 和 `docs/features/` 为准——**本文里的建议都还没实现，参考项目做过也不等于 Kiana 要做**。

> 硬约束说明：下面所有建议都不新增模型可见工具（仍然只有 shell / apply_patch / mcp / memory.search / memory.write 五个），不引入第二条执行路径，副作用一律过 ControlPlane，fail-closed。违反这几条的原始发现单独列在第六节。P0 按你的要求控制在 8 条；第五节的审计维护单列「紧急 / 重要 / 常规」，不计入这 8 条。

## 零、决策记录（2026-09-08，产品主人授权「按推荐方案执行」）

第七节的 8 个问题已按下列推荐定案，不再逐个等确认：

1. **自动批准：保留但严格收窄。** 只允许 LocalWrite 一档；开关默认关闭；每次自动批准必须追加一条带「自动批准」标记的事件，收据里可见。更高风险一律弹审批。
2. **审批动作：首发三个。** 「批准这一次 / 拒绝并继续 / 拒绝并中止」先做；「本会话批准」「持久 prefix 规则」列为第二阶段（涉及持久化与撤销，工作量差一档）。
3. **跨进程恢复：默认暂停（fail-closed）。** 重启后重建待审批列表，但不自动续跑；用户显式点「恢复」才继续。
4. **检查点 / 回滚：首发状态层 + 编辑级 undo。** 复用现有 apply_patch 的前置快照能力；文件层 shadow git 列为第二阶段。
5. **任务图：首发就做。** 依赖边用显式 `WorkPacket.dependencies` 字段，不从 packet 文本解析；`ready_packets` 与 PathLock 的关系写死「规划期检查 + 运行期兜底」。
6. **记忆候选可见性：分层处理。** instance-scratch 层保持默认可见（它是临时草稿），持久层默认 candidate 不可检索——避免现有 Builder scratch 写入在测试里突然消失。
7. **重审顺序：先 codex（审批 P0）→ deepseek-harness（P0 主参考）→ grok-build（检查点 / 确定性）。** 其余按 S-1 清单排后。
8. **参考矩阵：接受轻量维护。** 结构化审计 + 最后核验日 + 只核对被引用路径，不做每次全量重审。

执行口径两条：

- 规范落点只写进 `docs/company-os-*.md`（规范层）。`docs/features/*.md` 是「代码现状」层，**不改写成目标**，最多加一句指向本建议书。
- 审计维护：S-4 / S-5 本轮就做（矩阵与审计 README 加最后核验日）；S-1 / S-2 / S-3 / S-6 的完整重审列为待办，本轮不执行。

## 一、先说结论（按优先级）

1. **P0 — 审批在三个界面都能点。** 现在 TTY、Web、一次性 CLI 都不能批准，审批只是序列化进错误字符串；带 `--approve-local-write` 时本地写会被自动批准。先把它做成协议里的一等请求 / 应答，并让未决审批对所有界面可见、重启后还在。
2. **P0 — 审批按钮说人话，拒绝不等于终止。** 把「批准这一次 / 本会话 / 持久规则」「拒绝并继续」「拒绝并中止」拆成明确动作，拒绝时可以附一句话让模型换个办法。
3. **P0 — 审批决定写进事件账本。** 谁、在什么范围、批 / 拒了什么、什么时候过期，都要有一条用户看得见、重启后还在的记录，而不是只记「开了一张单」。
4. **P0 — 续跑材料落盘。** 把待审批的 CapabilityRequest、RunSnapshot、invocation_id + attempt + 序号存下来；现在重启后点批准只能拿到「续跑材料丢失」。
5. **P0 — 用 EventLog 重放重建运行态。** 内存里的 sessions / pending 表降级成「可重建的缓存」，重启后系统知道哪些 run 还在跑、哪些在等审批。
6. **P0 — 任务图有唯一的「就绪」定义，依赖缺失 / 成环在启动前就拦下。** 现在 board、swarm、domain 三处各算一套 ready，`WorkPacket.dependencies` 基本是死字段。
7. **P0 — 模型写记忆默认只是候选，来源由服务端判定，不能自批。** 现在 `memory.write` 的 source / promote_to 由模型自己传，模型可以把任意文本变成可检索记忆。
8. **P0 — 技能 / 扩展的声明只是声明，不是授权。** allowed-tools 只能当「少弹几次窗」的预批准；扩展清单要补 effect、所需能力、内容摘要，安装时校验。
9. 紧接着（P1）：检查点 / 回滚（现在完全没有）、可恢复的 Web 事件桥（游标 + 重连）、运行状态讲清楚（重试倒计时、正在取消）、编辑后自动验证。
10. 审计维护：codex / goose / adk-python / deepseek-harness / agent-framework 的旧审计已明显落后，建议优先重审；grok-build 值得补一份新审计；参考矩阵要补 30+ 条目（详见第五节）。

## 二、交互（Interaction）

**I-1（P0）审批变成三个界面都能应答的一等请求**

- **参考了谁**：codex（`codex-rs/app-server-protocol/src/protocol/common.rs:1728-1755`、`protocol/v2/item.rs:1533-1600`，审批是 server→client 的 ServerRequest，带 available_decisions）；opencode（`packages/opencode/src/permission/index.ts:67-185`，pending 队列任何客户端都能列举）；OpenHands / goose（`src/components/shared/buttons/conversation-confirmation-buttons.tsx`、`ui/desktop/src/acp/permissionRequests.ts`，审批内联在对话流里，用 generation id 防重复提交）；spec-kit（`src/specify_cli/workflows/steps/gate/__init__.py`，非 TTY 时返回 PAUSED 而不是自动通过）。
- **Kiana 现在什么样**：TTY 只认 `/trust /sandbox /receipt /cancel /quit`；Web 没有审批路由；一次性 CLI 把 challenge 序列化进 `control_plane_command_awaiting_approval:{json}`，或对本地写走 `should_auto_approve_local_write` 自动批准（`docs/company-os-ui-ux.md` §5.3.1、`docs/features/05-approvals.md`）。
- **覆盖状态（已覆盖，只补落点）**：目标交互本身 `docs/company-os-ui-ux.md` §5.3 / §5.3.1 已写成规范（含「三个界面当前都没有完整审批」的诚实描述），本条不新增目标，只补协议形状与 DaemonHost 的落点。
- **建议改什么**：在 `kiana-protocol` 增加审批请求 / 应答（单号、对象、available_decisions、有效期），由 DaemonHost 发出、三个界面渲染并回复，ControlPlane 负责 resolve；pending 列表三个界面共用，重启后从落盘的审批单恢复列表（只恢复单子，不恢复内存执行态，并明确标注需要重新发起）；非交互场景默认暂停、必须显式恢复。传输保持进程内即可，只改协议形状。
- **改哪份文档**：`docs/company-os-ui-ux.md` §5.3 / §5.3.1 / §6.1.1；`docs/features/05-approvals.md`；`docs/features/08-surfaces.md`；`docs/company-os-platform-architecture.md`（审批一节）。

**I-2（P0）审批按钮的语义分层：拒绝 ≠ 终止，拒绝可带理由**

- **参考了谁**：codex（`codex-rs/tui/src/bottom_pane/approval_overlay.rs:829-1110` 每个选项标签直接写清后果与范围，`tui/src/keymap.rs:434-475` 五类决定各配快捷键）；roo-code（`webview-ui/src/components/chat/ChatView.tsx:262-403`、`src/core/assistant-message/presentAssistantMessage.ts:198-207`，按 ask 类型映射动作对，拒绝会编码成带用户反馈的 tool_result 回灌模型）；opencode（`permission/index.ts`，拒绝带 message 时把反馈交回模型而不是打死 run）。
- **Kiana 现在什么样**：只有抽象的「批准 / 拒绝」，无法区分「拒绝这个动作」和「终止整个任务」；拒绝不能附理由，runner 只收到 `approval_denied` 失败结果（`docs/features/05-approvals.md` 主路径第 10 步）。
- **覆盖状态（部分已覆盖）**：`docs/company-os-ui-ux.md` §5.3 已有「拒绝 / 批准这一次 / 批准此项目内同类（仅当 policy 允许）」的按钮分层与「已消费 / 拒绝 / 过期 / 取消」的消费语义；本条新增的是「拒绝 ≠ 终止、拒绝可带理由、取消才算终止 run」这部分。
- **建议改什么**：在 domain 定义封闭的决定集：批准这一次 / 本会话批准 / 带 policy 修订批准（仅 policy 明确允许时）/ 拒绝但继续 / 拒绝并中止（作用域再细分 turn / session / policy 见 A-4）。每个选项标签写清后果与范围（例如「以后不再询问以 X 开头的命令」），并绑定独立快捷键；Deny 允许输入一句理由，作为带 feedback 的 tool result 回灌模型；运行中的 shell 另给「继续 / 终止」两个动作。拒绝只拒绝当前 invocation，cancel 才是终止 run。
- **改哪份文档**：`docs/company-os-ui-ux.md` §5.3；`docs/features/05-approvals.md`；`docs/features/09-session-lifecycle.md`（取消语义）；`docs/features/02-tool-call-spine.md`。

**I-3（P0）审批决定写成一条带 actor 和范围的事件卡**

- **参考了谁**：codex（`codex-rs/tui/src/history_cell/approvals.rs:40-200`，把「this time / every time this session / always run commands that start with …」渲染成历史卡；`core/src/session/handlers.rs:172-210`，先持久化 amendment 再通知等待者）。
- **Kiana 现在什么样**：EventLog 记了审批被 stage，但没有一条 durable、用户可见的「谁在什么范围批 / 拒了」回执；审批状态在内存，重启即失（`docs/features/05-approvals.md`、`docs/features/06-eventlog-receipts.md`）。
- **建议改什么**：新增审批决定事件，字段含 actor（user / reviewer）、scope（once / session / policy）、subject（命令 / 路径 / host）、expiry、结果（消费 / 拒绝 / 过期 / 取消），由 ControlPlane 追加进 EventLog 并投影到 transcript 和 Receipt；拒绝与中止必须是不同终态。这样界面才能满足 UI/UX §5.3.1 要求的「批准结果必须显示已消费、拒绝、过期或取消」。
- **改哪份文档**：`docs/features/05-approvals.md`；`docs/features/06-eventlog-receipts.md`；`docs/company-os-ui-ux.md` §5.3.1。

**I-4（P1）审批卡固定解剖：摘要 + 风险条 + 一键展开完整 diff / 命令**

- **参考了谁**：codex（`tui/src/app/event_dispatch.rs:2996-3045` 全屏展示完整 diff / 命令，`tui/src/diff_render.rs:1-30` 统一 diff 渲染，`approval_overlay.rs:539-560` 独立的全屏快捷键）；OpenHands（`src/components/shared/risk-alert.tsx` 风险等级横幅）；cline（`sdk/packages/ui/components/agent-approval-card.tsx` 固定解剖：标题 / 详情 / 元数据 / 错误槽 / 双按钮 / 进行中态）；roo-code（`webview-ui/src/components/chat/BatchDiffApproval.tsx`，多文件聚合成一张卡，逐文件折叠）。
- **Kiana 现在什么样**：三个界面都没有 diff 展示，审批 payload 只有摘要（`docs/company-os-ui-ux.md` §5.3.1）；多文件改动会变成多张单。
- **覆盖状态（部分已覆盖）**：`docs/company-os-ui-ux.md` §5.3 与 §2.3 已要求审批卡包含 exact target / payload 或 diff / 风险等级 / 有效期；本条新增的是全屏 diff、多文件聚合与逐文件折叠。
- **建议改什么**：审批卡先给紧凑摘要（目标文件 / 命令 / 原因 / 风险 / 有效期），一个按键打开全屏看完整 unified diff（行号、增删符号、语法高亮）或完整命令；风险等级必须来自 policy / gates，不能由界面猜；多文件补丁聚合成一张卡，逐文件折叠 diff 和 +/− 统计。全屏只是投影，复用已 stage 的 payload，不额外执行任何东西。
- **改哪份文档**：`docs/company-os-ui-ux.md` §5.3 / §6.2 / §2.3；`docs/features/05-approvals.md`；`docs/features/03-five-tools.md`（apply_patch）。

**I-5（P1）多会话隔离、后台状态徽标、跨会话挂起审批提示**

- **参考了谁**：goose（`ui/desktop/src/components/ChatSessionsContainer.tsx` 全部挂载 + CSS 隐藏 + 保持后台流不断，`acp/chatSessionStore.ts` 按会话分区，`components/Layout/NavigationPanel.tsx` 状态徽标与未读点）；OpenHands（`src/stores/conversation-state-store.ts`、`use-event-store.ts` 按 conversation id 分区）；codex（`tui/src/bottom_pane/pending_thread_approvals.rs:1-80`，底部提示其他会话有挂起审批，可切换过去）。
- **Kiana 现在什么样**：Workbench 同时只能看一个活动 run；Web 仍持有进程内全局 `active_session`，多标签页隔离尚未证明（`docs/company-os-ui-ux.md` §7.1 / §7.3）；审批可能挂在非当前会话上，没有任何提示。
- **建议改什么**：客户端 store 全部按 run / session id 做 key，每个活动会话渲染成独立隐藏 pane，切换只改可见性；导航栏按 ControlPlane 投影显示「在跑 / 等你审批 / 刚完成未读」徽标，进入即清；底部列出有挂起审批的会话（最多 3 个 + 「…」）并支持跳转。跳转只切视图，裁决仍走该会话自己的审批请求，不允许一个会话代另一个会话批准。
- **改哪份文档**：`docs/company-os-ui-ux.md` §7.3 / §3.2；`docs/features/09-session-lifecycle.md`；`docs/company-os-platform-architecture.md` §4.1。

**I-6（P1）可恢复的 Web 事件桥：先拉尾页、再按游标增量、断线退避重连**

- **参考了谁**：OpenHands（`src/contexts/conversation-websocket-context.tsx`、`src/hooks/query/use-conversation-history.ts`、`src/api/event-service/event-service.api.ts`，REST 尾页做锚点 + `since` 增量 + 按事件 id 去重；`src/hooks/use-websocket.ts` 指数退避加抖动、旧 socket 不得覆盖新连接；`components/features/chat/error-message-banner.tsx` 连接类错误首次连上后才提示）；crush（`internal/workspace/workspace.go:51-73` 连接健康度两态 ConnectionDegraded / ConnectionRecovered，另带一个 Stuck 标志；`internal/ui/model/ui.go:1532-1533` 恢复后重新 loadSession）。
- **Kiana 现在什么样**：Web 事件面能展示事件和 receipt，但没有「客户端游标 + 增量订阅」这一层，重开页面基本靠重新拉全量，断线行为不可预期（`docs/company-os-ui-ux.md` §7.2）。
- **覆盖状态（主干已覆盖）**：`docs/company-os-ui-ux.md` §7.2 与 `docs/company-os-platform-architecture.md` §10.2 已规定「cursor + snapshot + 合并 after cursor + epoch fence」；本条新增的是 REST 尾页锚点、事件 id 去重、退避参数与错误分类。
- **建议改什么**：打开页面先用 REST 取最近 N 条做锚点，再用 `after_event_id` 订阅增量，投影层按事件 id 去重，重放事件只更新事实、不重复触发通知 / 未读等副作用；断线显示「正在重连」并退避重试（约 1s 起、30s 封顶、加抖动），恢复后从 EventLog 重载并明确告知「断线期间的事件可能缺失，已按账本重建」；错误分 connection / conversation / auth 三类，连接类下一条正常事件自动清除，auth / 信任类保持粘性。Web 仍必须 loopback-only。
- **改哪份文档**：`docs/company-os-platform-architecture.md` §10.2；`docs/company-os-ui-ux.md` §7.2 / §11；`docs/features/06-eventlog-receipts.md`。

**I-7（P1）运行状态讲清楚：per-session 状态机、重试倒计时、正在取消**

- **参考了谁**：opencode（`packages/schema/src/session-status-event.ts` 的 idle / busy / retry，`src/session/status.ts:27-56` per-session Map，`src/session/retry.ts:180-205` 带 attempt / message / action / next，`packages/tui/src/component/prompt/index.tsx:1516-1600` 基于 next 时间戳倒计时）；crush（`internal/proto/proto.go` 的 RunComplete 终局信封，带 SessionID / RunID / MessageID / Text / Error / Cancelled）；OpenHands（`src/stores/optimistic-user-message-store.ts` 的发送中 / 失败 / 重试 + 看门狗）。
- **Kiana 现在什么样**：`ExecutionStatus` 有九个状态，但界面呈现基本是「转圈 / 不转圈」；重试和卡死分不清；输入提交后没有本地 pending 气泡，也看不出消息是已送达还是失败（`docs/features/09-session-lifecycle.md`）。
- **建议改什么**：把 per-session 运行状态提升为一等可观察状态（idle / running / awaiting_approval / cancelling / retry），retry 带结构化信息（第几次、下一次时间戳、可读原因、可选行动），会话列表和状态栏据此渲染，倒计时用服务端时间戳而非本地计数；定义单一终局信封 `run.finished`（含 run_id / status / error / cancelled / 最终 assistant 文本），三个界面都以「run_id 匹配的终局信封」作为结束条件，并用内嵌文本对账已渲染内容；用户消息提交后先插一条 sending 气泡，超时或失败翻成 error 并给重试。
- **改哪份文档**：`docs/company-os-ui-ux.md` §3.2 / §6.5 / §5.2；`docs/features/09-session-lifecycle.md`；`docs/features/06-eventlog-receipts.md`。

**I-8（P1）取消要两段式确认，取消后把半截历史收拾干净**

- **参考了谁**：crush（`internal/ui/model/ui.go:4362-4415`，第一次 Esc 只置 isCanceling 并起 2 秒计时器，第二次才真取消）；opencode（`packages/opencode/src/session/prompt.ts:1203-1225` 给未完成的 assistant 补结束状态，`prompt.ts:360-378` 把仍在 running 的工具改成 Cancelled）；continue（`gui/src/redux/thunks/cancelStream.ts` 的 setInactive + abortStream + clearDanglingMessages，`slices/sessionSlice.ts:279-340` 清理悬挂工具调用，本轮没产出就把原输入回填）；mini-swe-agent（`src/minisweagent/agents/interactive.py:109-122`，中断时允许输入一句 comment 后继续）。
- **Kiana 现在什么样**：取消是一次性、无确认的；取消后对话历史里正在进行的 assistant 步骤 / 工具调用如何收尾没有规范；`harness.steer` 机制存在但入口层没有任何命令走到它（`docs/features/09-session-lifecycle.md`）。
- **建议改什么**：CLI / TTY 用两段式取消（首次提示「再按一次取消」，超时自动复位），Web 用显式确认；取消收敛时统一收尾——把仍在执行的能力调用标记为 cancelled、给当前 assistant 步骤补结束时间和状态、界面用弱化样式显示「已中断」；如果本轮没有任何有效产出，把用户原始输入回填输入框；取消时可给一个可选输入框，把一句话作为用户消息注入当前 run 继续（不输入则维持取消）。所有后续工具调用仍要过 ControlPlane。
- **改哪份文档**：`docs/company-os-ui-ux.md` §5.4 / §5.4.1；`docs/features/09-session-lifecycle.md`；`docs/features/08-surfaces.md`。

**I-9（P1）工具调用用显式五态渲染，Reviewer 拿到逐文件 diff 而不是文件名清单**

- **参考了谁**：crush（`internal/ui/chat/tools.go:33-42` ToolStatus 五态 AwaitingPermission / Running / Success / Error / Canceled，状态从结果派生，转圈只由状态驱动）；roo-code（`webview-ui/src/components/common/CodeAccordion.tsx`、`src/core/diff/stats.ts` 后端算 +/− 统计并归一化，`components/chat/FileChangesPanel.tsx` 会话级聚合 original vs final）；cline（`apps/vscode/webview-ui/src/components/chat/DiffEditRow.tsx`）。
- **Kiana 现在什么样**：工具 / 能力调用按事件临时渲染，容易用多个布尔量拼状态，出现「转圈但早已失败」「已取消还显示进行中」；Reviewer 裁决只看改动文件清单（`docs/features/07-roles-departments.md`）。
- **建议改什么**：为每个工具 / 能力调用定义显式状态枚举（等待审批 / 执行中 / 成功 / 失败 / 取消），渲染状态由枚举 + 结果派生，并与 domain 的 `ExecutionStatus` 保持映射而不是重复定义；定义 DiffArtifact 投影（后端算统计、前端逐文件折叠 + 行号 + 跳转，会话级聚合总 +/−），让 Reviewer 的输入从「文件清单」升级为「带统计的逐文件 diff」。diff 只读展示，路径越界必须在展示前 fail-closed。
- **改哪份文档**：`docs/company-os-ui-ux.md` §3.2 / §3.3 / §5.4；`docs/features/07-roles-departments.md`；`docs/features/03-five-tools.md`。

## 三、功能（Feature）

**F-1（P0）模型写记忆默认只是候选，来源由服务端判定，不能自批**

- **参考了谁**：memorix（`src/knowledge/claims.ts:120-135`，origin=model 时强制 reviewState=draft、status=unknown，不能 approved/active；`claims.ts:250-289`，reviewClaim 是显式 approve/reject，注释写明 agent 的观察不能自己跨过这道边界；`src/memory/long-term.ts:429-435`，qualify 和 approve 是分开的、必须显式调用的状态跃迁）。
- **Kiana 现在什么样**：`memory.write` 是模型可见五工具之一，模型自己传 collection / text / source / promote_to（`kiana-runner/src/tools.rs:171-186`）；`kiana-daemon/src/harness_memory.rs:157-205` 直接 append，没有来源分类、没有 candidate 状态；带 `--approve-local-write` 开关时 LocalWrite 还会自动放行（`kiana-entrypoints/src/command_dispatch.rs:57-63`）。
- **覆盖状态（部分已覆盖）**：`docs/company-os-platform-architecture.md` §16 #4 已把「本地版 Memory promotion 审批主体 / 是否允许自批」登记为开放决策；本条新增的是 origin 由服务端派生、candidate 默认不可检索的具体字段。
- **建议改什么**：`MemoryRecord` 增加服务端派生的 origin（model / hook / git / user，不读模型传的 source）和 review_state；模型经 `memory.write` 写入一律落 candidate + draft，默认检索排除；只有操作者 / 目标层 owner 显式批准（走现有本地命令或审批项）才转 active。注意区分 instance-scratch 与持久层，避免现有 Builder scratch 写入在测试里突然不可见。
- **改哪份文档**：`docs/company-os-platform-architecture.md` §5.3 Memory 写入流程；`docs/features/10-context-search.md`；`docs/company-os-security-constitution.md` SEC-11。

**F-2（P0）技能 / 扩展的声明只是声明，不是授权**

- **参考了谁**：OpenSpec（`src/core/shared/allowed-tools.ts` 注释明确 allowed-tools「只预批准——不限制」，其它工具仍走正常权限；`skills/openspec-propose/SKILL.md` 的 allowed-tools 只是减少逐次提示）；spec-kit（`extensions/template/extension.yml` 的 effect 字段（read-only / read-write，模板里是注释示例）、requires.tools、hooks；`extensions/EXTENSION-PUBLISHING-GUIDE.md` catalog entry 带 sha256，安装前校验；`src/specify_cli/_download_security.py` 路径穿越 / symlink / 大小校验）。
- **Kiana 现在什么样**：`kiana-skills` 解析并保存了 frontmatter 的 allowed_tools，但没有任何地方把它变成执行边界；技能正文只是被注入 System 上下文。`kiana-types/src/plugin.rs` 的 `PluginManifest` 只有 name / version / description / author + 任意 extra，没有 effect、没有 required_capabilities、没有摘要校验。
- **建议改什么**：写死规则——skill 的 allowed-tools 只影响提示 / 展示，不进入 policy，技能想用任何能力都必须和其它模型工具一样经 broker + policy + approval；补一条 fail-closed 测试：声明 `allowed-tools: [shell]` 的 skill 不能导致任何未经批准的 shell 执行。扩展 / 插件清单增加 effect（read-only / read-write）、required_capabilities、network_policy、content_hash / signature、requires（版本 / 能力），安装时校验摘要与兼容性，read-only 扩展的写操作在 broker 层直接拒绝。**声明不等于授权，安装成功也不等于安全验证完成。**
- **改哪份文档**：`docs/company-os-security-constitution.md`（INP-01 边界补充）；`docs/company-os-quality-ecosystem.md` §9.1 / §9.2 / §10；`docs/features/03-five-tools.md`；`docs/features/05-approvals.md`。

**F-3（P1）记忆生命周期：准入、证据、可信度、双时态失效（valid_from / valid_to 两套时间）、保留衰减、读和管分开的授权谓词**

- **参考了谁**：memorix（`src/memory/admission.ts:19-56` candidate / ephemeral 不进默认投递；`src/memory/long-term.ts:58-74` / `:406-408` 无 evidence 不能进 qualified / approved；`src/memory/disclosure-policy.ts:22-105` 从 commit / sourceDetail 派生 evidence basis；`src/memory/visibility.ts:39-78` canRead 与 canManage 是独立谓词）；graphiti（`graphiti_core/edges.py:254-284` valid_at / invalid_at / expired_at，`utils/maintenance/edge_operations.py:538-573` 新事实把旧 edge 置失效但保留历史）；MemPalace（`src/storage/sqlite.ts:32-42` triples 表 valid_from / valid_to，`src/mcp/server.ts:232` 显式失效工具）；mem0（`mem0/memory/main.py:1026-1035` 内容 hash 去重，ADD / UPDATE / DELETE 生命周期）。
- **Kiana 现在什么样**：`MemoryRecord` 只有 source 字符串，verified 仅判断 source 非空，模型传 source=anything 就能让命中显示 verified=true；检索是全部 query term 子串包含，没有分词 / 向量 / 图 / 融合；没有到期、访问计数、衰减、失效或去重（`docs/features/10-context-search.md` 短板）。
- **建议改什么**：加 admission_state（candidate / qualified / ephemeral，与生命周期 status 正交），默认检索只返回 qualified 和 legacy（无字段的旧记录按 legacy 保持可见，避免升级后记忆消失）；证据建成独立记录（kind / referenceId / relation，含 contradicts），晋级必须引用至少一条 supports / verifies；由捕获通道派生 provenance basis 与 confidence，检索排序用它们但授权不用；事实型记忆加 valid_from / valid_to / expired_at，冲突时旧记录标失效并 append 事件，检索默认过滤已失效；保留期用确定性 relevance 与衰减做 sweep，免疫规则显式排除 candidate / ephemeral；抽出 fail-closed 的 can_read / can_manage 两个谓词（能读 ≠ 能管，两个判断各自独立、默认拒绝），统一所有检索与写入。向量 / 图检索可以作为后续项，但分数只进排序、不参与授权。
- **改哪份文档**：`docs/company-os-platform-architecture.md` §5.3；`docs/company-os-quality-ecosystem.md` §9；`docs/company-os-operations-governance.md` §9.3；`docs/features/10-context-search.md`。

**F-4（P1）编辑后自动验证 + 有上限的反思回灌**

- **参考了谁**：aider（`aider/coders/base_coder.py:1599-1623` 编辑后 auto_lint 跑 lint，出错先问一句「要修吗」，用户同意才把 lint_errors 作为 reflected_message 回灌，auto_test 同形；`base_coder.py:924-944` 反思外层循环 max_reflections=3，超过就停；`aider/args.py:543-563` --auto-lint 默认 True、--auto-test 默认 False）；mini-swe-agent（`src/minisweagent/config/mini.yaml:9-19` 推荐工作流：先复现、改代码、再跑验证、再测边界）。
- **Kiana 现在什么样**：`apply_patch` 成功即返回，runner 没有编辑后验证，全仓没有 auto_lint / auto_test / reflect 之类的实现；模型可以只声明「改好了」而从不真正跑测试（`docs/features/02-tool-call-spine.md`）。
- **建议改什么**：在 runner / daemon 层加一个编辑后验证钩子：workspace-write 类能力成功后，可选执行项目配置的 verify 命令（lint / test），把输出作为一条 observation 追加；失败时回灌并限次重试（例如 3 次），超限记事件并停止。命令必须走既有 capability broker / ControlPlane 通道，不是第二条执行路径；工作台 / Web 暴露 verify_commands 配置和开关。
- **改哪份文档**：`docs/features/02-tool-call-spine.md`；`docs/features/01-model-execution.md`；`docs/company-os-platform-architecture.md`（执行路径一节）。

**F-5（P1）工具输出按头尾截断并显式告知省略量；apply_patch 失败给可行动的反馈**

- **参考了谁**：mini-swe-agent（`config/mini.yaml:112-128`，输出超 10000 字符时给 head 5000 + tail 5000 + elided_chars + 「Output too long」提示；`agents/default.py:154-157`、`environments/local.py:24-43` 固定结果形状）；aider（`coders/editblock_coder.py:82-124` 回显失败的 SEARCH/REPLACE 块、给出「Did you mean to match these actual lines?」、告知哪些块已成功「Don't re-send them」；`base_coder.py:1596-1607` 把匹配错误变成 reflected_message 回灌）。
- **Kiana 现在什么样**：`kiana-runner/src/harness.rs:525-530` 的 tool_result_text 把 output 原样返回，只有压缩阶段做中段截断；`kiana-daemon/src/apply_patch.rs` 的失败只返回机器码（`apply_patch_hunk_mismatch`、`apply_patch_hunk_ambiguous` 等），没有上下文回显、没有近似行提示、不区分哪些 hunk 已成功。
- **建议改什么**：在把观察写进模型对话前做确定性的 head / tail 截断，附 elided_chars 和提示，完整输出仍写进 EventLog / receipt 供审计；补丁失败在保留机器码前缀（测试依赖）的前提下，把目标路径、失败 hunk 的上下文、文件中最接近的匹配行、本次已成功的 hunk 列表补进错误消息，并提示「不要重发成功的块」。两者都要有长度上限、确定性、可回归测试。
- **改哪份文档**：`docs/features/02-tool-call-spine.md`；`docs/features/06-eventlog-receipts.md`；`docs/features/03-five-tools.md`。

**F-6（P1）repo map 从「按路径截断」升级为「按相关性排序 + 预算装箱」，并用消息里的标识符选上下文**

- **参考了谁**：aider（`aider/repomap.py:365-574` 用 def / ref 建图跑 PageRank，chat 文件 ×50、提到的标识符 ×10；`repomap.py:629-706` 对 ranked_tags 二分搜索逼近 max_map_tokens；`coders/base_coder.py:678-707` 从消息抽标识符并映射到同名文件，`base_coder.py:1714-1760` 从消息识别文件提及）。
- **Kiana 现在什么样**：`kiana-query/src/repo_map.rs` 按稳定路径排序后顺序纳入，预算不够就丢弃尾部整份文件并置 truncated / omitted_files，是「路径序 + 截断」而不是相关性排序；repo map 和 context pack 目前主要是人用，模型搜代码走 shell 跑 rg（`docs/features/10-context-search.md`）。
- **建议改什么**：从当前消息抽取标识符 / 文件名，对定义这些符号的文件加权，预算内按分数装箱而不是按路径顺序截断；把命中文件作为种子加入 context pack 供模型使用；保留确定性 tie-break 和现有 truncated / omitted 统计，先做本地符号索引，不引入 embedding / 网络依赖。
- **改哪份文档**：`docs/features/10-context-search.md`；`docs/reference-agent-audit/04-aider.md`（补 Kiana 采纳点）。

**F-7（P1）工件链：声明式 DAG + validate 门禁 + 机器可读安全宪法 + 冻结的验收检查**

- **参考了谁**：OpenSpec（`schemas/spec-driven/schema.yaml`，artifacts 各带 generates / requires，apply.requires 决定何时允许进入实现；`src/commands/validate.ts` 的 --strict / --json / 稳定 issue code；`src/utils/requirement-diff.ts` 需求级 diff）；spec-kit（`templates/commands/plan.md` / `analyze.md`，Constitution Check 是必须通过的 GATE，冲突自动 CRITICAL；`workflows/steps/gate/__init__.py` 非法配置直接 FAILED）；architect-loop（`skills/frozen-checks/SKILL.md`，`command -> exit:<n>` 的冻结检查，冻结后编辑自动 FAIL）；beads（`internal/types/types.go` 的 AcceptanceCriteria）。
- **Kiana 现在什么样**：`kiana-workflow` 只有一张状态转移表，没有「工件存在性 + 依赖边」的声明式图；安全宪法是文档而不是 plan / analyze 能读取并强制校验的输入；`kiana project` 有 board / next 但没有可被控制面和界面共同消费的 validate 命令；Reviewer 裁决只是数 Builder 改了哪些文件（`docs/features/07-roles-departments.md`）。
- **建议改什么**：在 `kiana-workflow` 落一个版本化 ArtifactGraph（每个工件声明 id / generates / template / requires），apply 只认 apply.requires 的传递闭包，缺依赖即 blocked；把硬约束（五工具、冻结项、单执行路径、fail-closed、审批不绕过）抽成机器可读的 constitution，plan / analyze 节点逐条给出 pass / violation，violation 直接阻断；加 `kiana workflow validate --json` 只读门禁（稳定 issue code + 退出码）和只读的跨产物一致性分析；packet 绑定冻结的 check 文件（命令 + 期望退出码 / 输出），Builder 执行后写进 EvidencePacket，Reviewer / Closer 只读结果判 pass / fail，冻结后改动即 FAIL。这些命令仍经现有 shell 能力在 policy / sandbox 下执行，不新增模型可见工具。
- **改哪份文档**：`docs/company-os-platform-architecture.md` §8.2 / §8.3 / §8.4；`docs/company-os-security-constitution.md`（增加机器可读条款与校验契约）；`docs/company-os-quality-ecosystem.md` §9.1；`docs/features/07-roles-departments.md`；`docs/company-os-implementation-outline.md` Slice D。

## 四、架构（Architecture）

**A-1（P0）续跑材料落盘：pending CapabilityRequest、RunSnapshot、invocation 身份**

- **参考了谁**：temporal-sdk-python（`temporalio/worker/_workflow_instance.py:3336-3415` 把 activity 类型、参数、超时、重试策略写进 command；`:875-922` 按 seq 从 history 恢复 pending activity；`:2589-2592` / `:3271-3334` 用单调 seq 关联 pending 与 resolve）；openai-agents-python（`src/agents/run_state.py:764` 可序列化 RunSnapshot 是 HITL 的 durable pause / resume 边界；`src/agents/_tool_invocation.py:282` 对 type / name / arguments 做规范化 SHA-256 指纹；`run_state.py:1366` call_id→fingerprint / executed 随快照持久化）。
- **Kiana 现在什么样**：审批被暂存时只写 `approval.requested`，载荷里没有原始 CapabilityRequest、RequestContext 快照、sandbox 和事件游标；`PendingInvocation` 类型注释明确「故意不序列化」，只存在内存。重启后 `decide_approval_with_proof` 能校验 proof，但拿不到 Runner continuation，只能返回 `approval_continuation_unavailable`（`docs/features/05-approvals.md`、`docs/features/09-session-lifecycle.md`）。
- **建议改什么**：在审批暂存前先写一条 invocation 请求事件（或扩展 run.capability_requested），携带经 `redact_event_value` 处理后的 CapabilityRequest、invocation_id、attempt、policy_snapshot、sandbox 和事件游标；PendingInvocation 改为该事件的投影，decide_approval 从事件重建。同时在 domain 层定义 RunSnapshot（run_id / session_id、消息历史、pending_capability{call_id, args_fingerprint}、approval_decisions、step_counter、schema_version），由 ControlPlane 写入，启动时由 ControlPlane 调 resume_run 重建 harness 状态，runner 自身永远不读盘恢复。再加一份由 ControlPlane 独占写入的调用账本，键为 (run_id, call_id)，派发前查账本，已执行直接返回缓存结果或 fail-closed，指纹不一致立即拒绝。
- **改哪份文档**：`docs/features/05-approvals.md`；`docs/features/09-session-lifecycle.md`；`docs/company-os-design.md` §10.1；`docs/company-os-platform-architecture.md` §4.1 / §4.4 / §10.1；`docs/company-os-domain-contracts.md`。

**A-2（P0）用 EventLog 重放重建运行态，内存表降级为可重建缓存**

- **参考了谁**：temporal-sdk-python（`temporalio/worker/_workflow.py:344-396` 缓存缺失时由 history 重建实例，`:535-604` 实例可从内存驱逐而系统仍正确；`_workflow_instance.py:448-604` activate 按 job 重放）；Kiana 自己已有 `EventStorePort::read_stream` / `read_all` 与 CAS / 幂等键（`kiana-ports/src/lib.rs:241-342`）。
- **Kiana 现在什么样**：EventLog 已是带 CAS、幂等键和 aggregate stream 的事实源，但目前只用于 receipt 投影和按 approval_id 查游标；ControlPlane 的 `sessions`、`pending_invocations`、`cancellations` 是纯内存 `Mutex<HashMap>`，进程重启即清零，文档自己承认跨进程 resume 为 deferred。
- **覆盖状态（部分已覆盖）**：`docs/company-os-platform-architecture.md` §8.4 已写「workflow state 从 Event/State Store 重建」，§16 #2 已把跨进程恢复的持久化载体登记为开放决策；本条新增的是 RunProjection / InvocationProjection 的具体折叠形状。
- **建议改什么**：新增 RunProjection / InvocationProjection，用 `read_stream("run", run_id)` 与 `read_all` 折叠 run.* / capability.* / approval.* 事件，产出 Run / Invocation 的当前状态、待审批集合、未决 capability 集合；DaemonHost 启动时不主动全量恢复，而在首次按 run_id / session 访问时惰性重建（对应 Temporal 的 eviction + 重建）；折叠遇到互相矛盾的终态保持现有 `run_terminal_conflict` / `result_unknown` fail-closed 语义；内存 HashMap 改为 projection 的写穿缓存，不再是权威。重建出的 pending 项必须重新过 policy / gate / approval，不能直接交给 broker。
- **改哪份文档**：`docs/company-os-platform-architecture.md` §4.1 / §8.4；`docs/features/06-eventlog-receipts.md`；`docs/features/09-session-lifecycle.md`。

**A-3（P0）任务图：单一 ready 谓词、依赖缺失 / 成环 fail-closed、claim / lease 心跳回收**

- **参考了谁**：beads（`issueops/readyclaimer.go:6-30`，claim 的过滤条件就是 Reader.Ready 的同一个类型，注释明确「两个谓词一旦允许不同就迟早会不同」；`internal/storage/sqlbuild/ready.go:43-77` ready 是一段确定的 WHERE + ORDER BY；`issueops/blockedstate.go` blocked 是依赖图的不动点、含 parent-child 传递继承；`issueops/cycledetector.go:1-90` 输出规范化环、两次运行字节一致；`internal/storage/issueops/lease.go:17-36` DefaultLeaseTTL 5 分钟，worker 死后 `bd reclaim` 退回 ready）；gastown（`internal/convoy/operations.go:278-345` ready 定义：open、无 assignee、无未关闭 blocking 依赖、类型可派发）；claude-task-master（`scripts/modules/task-manager/find-next-task.js:24-130` eligibility = 状态合法 + 全部依赖 done；`scripts/modules/utils.js:1468-1509` DFS 找环）。
- **Kiana 现在什么样**：`kiana-tasks/src/project_board.rs:222-343` 的 Ready / Blocked 由外部 task JSON 的 source_status 推导，`swarm.rs:319` 又单独算一次 ready_tasks，`kiana-domain/src/lib.rs:1043` 的 `WorkPacket.dependencies` 只被渲染进 prompt；`kiana-core/src/lib.rs:947-976` 的 spawn 路径把 Draft→Approved→Assigned→Running 直接推完，从不检查依赖；全仓没有环检测，也没有 packet claim / lease（`docs/company-os-implementation-outline.md` Slice D 的「依赖缺失 fail-closed」验收在代码里没有对应判断）。
- **建议改什么**：在 `kiana-domain` / `kiana-tasks` 定义唯一的 `ready_packets(graph, now)`——状态属于可派发集合、dependencies 全部处于成功终态、无未过期 lease 冲突；ControlPlane 的 spawn 校验、`kiana project next`、Web / Desktop 看板都调用它，禁止各写一份。spawn 前校验依赖，未满足返回 blocked 并记事件；WorkPacket 增加派生 blocked 投影，父 blocked 则子 blocked；加 `validate_dependency_dag`（确定性规范化环，approve 和 workflow 模板注册时调用，失败拒绝落盘）；WorkPacket 增加 claim(owner, lease_expires_at, heartbeat_at)，spawn / continue / 每个 turn 续租，后台确定性扫描过期 lease 把 packet 退回 ready 并记事件。ready 只是查询，真正的执行许可仍由 ControlPlane 的 policy / gates / approval 产生。
- **改哪份文档**：`docs/company-os-implementation-outline.md` Slice D 验收；`docs/company-os-design.md` §5.1 / §9.2；`docs/company-os-domain-contracts.md`（依赖不变量）；`docs/features/07-roles-departments.md`；`docs/features/09-session-lifecycle.md`。

**A-4（P1）审批作用域四级阶梯：once / turn / session / policy**

- **参考了谁**：codex（`core/src/state/turn.rs:99-135` pending_approvals 与 granted_permissions 为 turn 级；`core/src/state/session.rs:370-395` granted_permissions 为 session 级、按 environment_id 合并；`core/src/tools/sandboxing.rs:45-125` ApprovedForSession 写缓存、后续相同 key 跳过提示；`execpolicy/src/amend.rs:65-110` 文件锁 + 追加 prefix_rule allow，`core/src/exec_policy.rs:466-500` 追加后重新评估 already_allowed 保证幂等）。
- **Kiana 现在什么样**：审批 / 授权状态全在内存，重启即失；没有持久 allow 规则；唯一的放行是 `--approve-local-write`，只覆盖本地写这一档（`docs/features/05-approvals.md`）。
- **建议改什么**：定义 once / turn / session / policy 四级 scope，每个决定路由到对应存储：turn 挂在 Run 状态，session 授权进程内按环境合并，policy 级落成 append-only 规则文件（写入加锁、幂等复查、可撤销、永不越出精确 prefix）。作用域必须写进审批决定事件，界面能显示「本会话有效 / 持久规则」。持久规则只在精确 prefix 内 allow，绝不泛化，且需要 policy 明确允许。
- **改哪份文档**：`docs/features/05-approvals.md`；`docs/features/04-trust-sandbox-path.md`；`docs/features/09-session-lifecycle.md`。

**A-5（P1）档位是「受约束的值 + 来源」，界面据此显示为什么不可用；无沙箱档二次确认**

- **参考了谁**：codex（`core/src/session/step_settings.rs:25-45` approval_policy 是 Constrained 且每 step 解析；`config/src/constraint.rs:63-175` Constrained::can_set 校验候选值；`config/src/config_requirements.rs:44-130` RequirementSource 与 ConstrainedWithSource；`tui/src/chatwidget/permissions_menu.rs:41-60` disabled_reason 来自 requirements.can_set；`permission_popups.rs:420-505` 无沙箱档二次确认，正文逐条写清后果）。
- **Kiana 现在什么样**：已有 read-only / workspace-write / danger-full-access 与 workbench 的 `/sandbox`，但没有「受约束值 + 来源」机制，无法说明某档位是谁钉死的、为什么不可用，也没有无沙箱档的二次确认（`docs/features/04-trust-sandbox-path.md`）。
- **建议改什么**：把 approval policy / permission profile 包成带来源（用户配置 / 项目 / 托管）的受约束值；界面用 can_set 渲染 disabled_reason（被 trust / policy 钉死的档位显示原因而不是隐藏）；更严格来源存在时，放宽策略的变更被拒绝；选择无沙箱 / full-access 档时弹确认对话框，正文明确「可修改任意文件、联网且不再逐次批准」，默认焦点在 Cancel。确认只影响 UX，授权仍由 ControlPlane 判定。
- **改哪份文档**：`docs/company-os-platform-architecture.md`（policy）；`docs/features/04-trust-sandbox-path.md`；`docs/company-os-ui-ux.md` §2.3 / §5.3 / §6.2。

**A-6（P1）重试与超时是可持久化的一等策略，错误分层，默认不重试**

- **参考了谁**：temporal-sdk-python（`temporalio/common.py:37-108` RetryPolicy：initial_interval / backoff_coefficient / maximum_attempts / non_retryable_error_types；`worker/_workflow_instance.py:3365-3394` 超时与 retry_policy 写进 command；`worker/_activity.py:606-630` activity 反序列化 heartbeat_timeout 与 retry_policy）；openai-agents-python（`src/agents/run_internal/model_retry.py:110-130` 只把网络类 / 超时 / 连接关闭归为可重试，并在重试前回滚会话）；pydantic-ai（`_tool_execution.py:1012` 区分 ToolRetryError（交回模型纠正）与 ToolFailed（终止）；`durable_exec/temporal/_agent.py:78-84` with_non_retryable_errors）。
- **Kiana 现在什么样**：规范里 WorkflowDefinition 已有 retry_policy / timeout_policy 字段，SupervisionLease 也有 retry_limit，但产品主路径把 `retry_limit` 写死为 0；CapabilityRequest 上没有超时 / 重试声明，broker 执行也没有 per-invocation 超时；一次模型调用失败整个 run 直接 Failed（`docs/features/09-session-lifecycle.md`）。
- **建议改什么**：给 CapabilityRequest / Invocation 增加可序列化的 RetryPolicy + TimeoutPolicy（初始间隔、退避系数、上限、不可重试错误类型、start_to_close / schedule_to_close 超时）；fail-closed 默认单次尝试，非幂等操作禁止重试；每次 attempt 写独立事件；超时或结果未知一律收敛为 result_unknown，绝不自动重试。在 runner 区分三类边界：①传输 / 模型瞬时错误 → 有界退避重试，且仅在尚无 capability 被执行时；②模型可纠正的工具错误 → 作为工具结果返回模型，不判 run 失败；③授权失败、指纹不符、账本显示已执行 → 绝不重试。重试前必须查调用账本。
- **改哪份文档**：`docs/company-os-platform-architecture.md` §8.2 / §4.4；`docs/features/02-tool-call-spine.md`；`docs/features/01-model-execution.md`；`docs/features/09-session-lifecycle.md`。

**A-7（P1）确定性合同：注入时钟 / 随机、logic_version 分支、Replayer 重放门**

- **参考了谁**：temporal-sdk-python（`worker/workflow_sandbox/_restrictions.py:48-70` 把非确定性访问变成硬错误；`worker/_replayer.py:38` / `:124` Replayer 用记录的历史重放并检出差分；`worker/_workflow_instance.py:1441-1468` workflow_patch 按已记录 marker 选分支；`common.py:1234-1251` VersioningBehavior PINNED / AUTO_UPGRADE）。
- **Kiana 现在什么样**：§8.5 写了「时间、随机数、request id 等必须来自可注入的 deterministic source」和 replay divergence 阻断规则，但 RequestContext 里没有可注入的时钟 / 随机源，也没有实现重放工具；`kiana-workflow` 只有一张无版本的状态转移表，policy / gate 语义变更后历史事件会被当前代码重新解释（`docs/company-os-platform-architecture.md` §8.5）。
- **覆盖状态（规范已覆盖）**：`docs/company-os-platform-architecture.md` §8.5 已写确定性合同（可注入时钟 / 随机、replay divergence、按已记录 marker 选分支），§16 #2 已登记持久化载体为开放决策；本条新增的是 RequestContext 字段与 `kiana replay` 的实现落点。
- **建议改什么**：在 RequestContext 注入 DecisionClock / RandomSeed / InputDigest，禁止 ControlPlane 读取环境时间与随机，把 digest 写进事件与 NodeExecution；引入 logic_version / patch marker 事件，fold 历史时先读已记录 marker 再决定走哪条分支，未知版本 fail-closed 拒绝重放而不是猜测；加 `kiana replay --run <id>`（仅 CI / 测试入口，不是模型可见工具）只读折叠该 Run 的事件，比对 (invocation_id, attempt, input_digest, 状态, error_code) 序列，首个不一致即 divergence point 并阻断 release。重放只做只读投影，绝不重跑副作用或调用模型。
- **改哪份文档**：`docs/company-os-platform-architecture.md` §8.4 / §8.5；`docs/company-os-quality-ecosystem.md` §5.1；`docs/features/06-eventlog-receipts.md`；`docs/company-os-spec-index.md`。

**A-8（P1）委派仍是 ControlPlane 的操作，不是第六个模型工具；有界 handoff + typed 失败**

- **参考了谁**：autogen（`python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_swarm_group_chat.py:47-80` handoff target 必须在 participant_names 内，`base/_termination.py:75-130` AND / OR 组合终止，`conditions/_terminations.py:60-100` MaxMessageTermination，`_base_group_chat_manager.py:195-246` 强制 max_turns；`_events.py:10-113` SerializableException / GroupChatError 的 typed 失败信封）；crewAI（`lib/crewai/src/crewai/tools/agent_tools/base_agent_tools.py:46-110` coworker 与 agents role 列表匹配，未知返回错误；`events/types/a2a_events.py:57-360` 委派事件谱系）；a2a（`specification/a2a.proto:167-208` Task / TaskState，`:244-278` Message 的 task_id / context_id）。
- **Kiana 现在什么样**：模型可见工具锁死 5 个，跨部门走 WorkPacket；`DelegationPacket` 有 delegation_allowed 与 grant 单调缩减，但规范没有把「委派」明确成 ControlPlane 操作面，也没有 turn / message 预算、显式终止谓词或 handoff 白名单；child failure 没有 typed payload，也没有「result_unknown 不得进 MergeDecision」的机器可检查规则（`docs/company-os-platform-architecture.md` §9.2 / §9.3）。
- **建议改什么**：明确 delegation 是 ControlPlane 的 assign / handoff 操作，不进入模型工具 schema，模型只能在 WorkPacket 内容里提出建议，实际目标由 ControlPlane 按 role / grant 白名单解析；`DelegationPacket` 增加 max_turns / max_messages / termination_predicate / handoff_allowlist，白名单来自 grant / template 而不是模型文本，超预算自动终止并写事件；定义 `ChildFailureReport{child_cell_id, reason_code, error_class, partial_output_refs, result_unknown, retryable, policy_snapshot}` 随 EventLog 持久化，MergeDecision 拒绝含 result_unknown 的 child 输出；加 delegation_started / completed / failed / reconciled 事件与 parent / child 关联。A2A 只取 Task / Message 的字段形状和 TaskState→Kiana 状态映射，不做远程 transport，不搬运原始 transcript。
- **改哪份文档**：`docs/company-os-domain-contracts.md`（DelegationPacket）；`docs/company-os-platform-architecture.md` §9.1 / §9.2 / §9.3 / §11.1；`docs/coding-pack-matrix.md` FZ-TEAM；`docs/company-os-security-constitution.md` SEC-09。

**A-9（P1）取消区分 try-cancel / wait / abandon，取消意图落盘；事件先落盘再推进**

- **参考了谁**：temporal-sdk-python（`temporalio/workflow/_activities.py:71-82` ActivityCancellationType：TRY_CANCEL / WAIT_CANCELLATION_COMPLETED / ABANDON；`worker/_workflow_instance.py:3417-3424` request_cancel_activity 带 seq，取消是对已知 pending 的操作）；adk-python（`src/google/adk/agents/invocation_context.py:275-296` / `runners.py:727-772`，non-partial 事件先 append 到 session、再 set 信号解除生产者等待，partial 事件不阻塞）。
- **Kiana 现在什么样**：取消只是进程内 watch 通道，没有 per-invocation 的取消类型，也没有把「谁请求取消、取消哪个 invocation」持久化；取消命令和 continue / steer 都是发给 harness 的进程内命令，进程一死未消费的输入就丢了；run 状态变更与事件落盘不是一个原子单元（`docs/features/09-session-lifecycle.md`）。
- **建议改什么**：先落 `run.cancel_requested` 事件（目标 invocation_id 列表 + cancellation_type），重启时从事件重放并重新下发取消；只有 broker 确认停止才写 cancelled，否则收敛为 result_unknown。在 ControlPlane 上区分三类 wire 意图：signal（continue / cancel / steer，先落事件再作用于 Run）、query（receipt / status，进入只读上下文，禁止写事件 / 派发）、update（审批决定，走 validator + handler 并返回结果）。让 run 状态变更走单一写入者：每次推进模型步之前，先把事件与状态 delta 作为一个持久化单元写入，runner await 到回执才继续；partial / 流式输出不参与阻塞。
- **改哪份文档**：`docs/company-os-platform-architecture.md` §4.2 / §4.4；`docs/features/09-session-lifecycle.md`；`docs/features/05-approvals.md`；`docs/features/08-surfaces.md`。

**A-10（P1）检查点 / 回滚：写前快照 + shadow git + 事务化恢复 + 时间线交互**

- **参考了谁**：roo-code（`src/services/checkpoints/ShadowCheckpointService.ts` 每任务独立 shadow git repo、独立 info/exclude、清洗 GIT_DIR 等环境变量；`src/core/checkpoints/index.ts:211-298` 写工具前和用户输入前 saveCheckpoint；`services/checkpoints/excludes.ts` 危险路径与嵌套 repo 防护；`webview-ui/src/components/chat/checkpoints/CheckpointMenu.tsx` 恢复拆成「只恢复文件」与「文件 + 对话」两个动作、二次确认 + 不可撤销红字）；cline（`sdk/packages/core/src/session/checkpoint-restore.ts:42-154` / `:376-478`，恢复前 git stash push --include-untracked 到私有 ref 形成可回滚事务，HEAD 已移动就拒绝 reset，只有捕获过 untracked 才 git clean -fd）；aider（`aider/commands.py:657-695` / `:553-600`，/diff 看自上次消息以来的改动，/undo 只回退本会话自己产生的提交）。
- **Kiana 现在什么样**：没有检查点 / 回滚，compaction 还是占位符（摘要写死 `(no summary available)`）；不过 `kiana-daemon/src/apply_patch.rs:529` / `:637` / `:646` 已有 capture_preconditions / rollback_preconditions / restore_snapshot，可作为 undo 的现成基础（`docs/features/09-session-lifecycle.md`）。
- **建议改什么**：设计 CheckpointService，绑定 transcript offset + workspace revision + invocation；在写工具前与用户输入前快照；恢复走 ControlPlane 并写事件 / Receipt，恢复后旧 approval 必须作废。恢复本身按高风险写操作处理：先快照当前工作区到私有位置，校验 HEAD 未移动才允许 reset，失败可回滚；untracked 清理只在确实被快照捕获时进行；沿用危险路径黑名单与嵌套 repo 检测，缺 git 时明确降级而不是假装可用。UI 上把 checkpoint 做成时间线里的一等对象，提供「预览 diff」「只恢复文件」「文件 + 对话」三个动作，破坏性动作二次确认并显著标注不可撤销；apply_patch 成功后产出结构化 diff 写进 receipt，并基于已有 precondition 快照提供编辑级 undo。preview 绝不能写盘，undo 是受控操作、不作为模型可见工具。
- **改哪份文档**：`docs/company-os-design.md`（checkpoint / 回滚节）；`docs/features/09-session-lifecycle.md`；`docs/company-os-ui-ux.md` §5.3 / §6.1；`docs/features/06-eventlog-receipts.md`；`docs/features/05-approvals.md`。

## 五、参考项目新鲜度

> 这一节是「审计维护」清单，不占前面 8 条 P0 的额度。判断依据是各参考仓库自审计日（2026-08-25/26）以来的 git log 与 `reference/` 目录现状。

**S-1（紧急）重审已明显过时的审计，优先这五份**

- **参考了谁 / 证据**（提交 / 文件数按 `git log --since=2026-08-25` 与首末提交 diff 实测，非精确到个位）：codex 落后约 640 提交 / 约 2600 个改动文件，新增 `core/src/guardian/`、`guardian_review.rs`、`ext/guardian-v2/`、`protocol/src/approvals.rs`、`windows-sandbox-rs/`、`codex-rs/worktree/`；goose 落后约 105 提交 / 约 500 文件，新增 `crates/goose/src/agents/state_machine/tool_confirmation.rs`、`crates/goose/src/agents/state_machine/ops_tool_approval.rs`、`crates/goose-roaming/`、`crates/goose-sdk/`，并移除了 fast model routing 与托管模型注册表；adk-python 落后约 205 提交 / 约 500 文件，`functions.py` 被拆分、tool call 拆成 prepare/execute、新增 YAML 图工作流；deepseek-harness 落后约 1690 提交 / 约 8100 文件（P0 主参考）；agent-framework 落后约 150 提交，含 SecretString 与 checkpoint 反序列化收紧的 BREAKING 变更。
- **Kiana 现在什么样**：这五份审计分别是 P0 的审批、Runtime、事件、Session/Turn ledger 首选参考，但引用的 file:line 已大面积漂移，agent-framework 的安全 BREAKING 还没反映进审计。
- **建议改什么**：按 P0 顺序重审 02-deepseek-harness 与 15-agent-framework，再重审 01-codex、07-goose、16-adk-python；codex 新增 Guardian approval 与 permission profile 交互、Windows sandbox 服务边界、worktree 生命周期三节，goose 复核「双循环是否仍并存」并更新 crate 边界图，adk 在新模块里复核「non-partial Event 先落库才继续」和「审批不可换 tool / call ID / args」两条核心结论。只取审批归属与状态机形状，不引入 Windows 服务化 / 远程 provisioning / 图编排主线。
- **改哪份文档**：`docs/reference-agent-audit/01-codex.md`、`02-deepseek-harness.md`、`07-goose.md`、`15-agent-framework.md`、`16-adk-python.md`；`docs/company-os-reference-matrix.md` §3。

**S-2（紧急）新增 grok-build 审计**

- **参考了谁 / 证据**：`reference/grok-build/crates/codegen/xai-workflow/src/journal.rs`（req_hash、稠密 seq、Divergence 报错、replay）与 `run.rs`（PauseKind、BudgetExceeded / Cancelled）；`xai-hunk-tracker/src/lib.rs`（Agent vs External hunk 归因）；`xai-fast-worktree/src/lib.rs`（CoW / btrfs 快照、worktree pool）；`xai-grok-sandbox/src/lib.rs`（Landlock / Seatbelt + 子进程 seccomp）；`xai-grok-session-events/src/lib.rs`（events.jsonl、EventTracker）。
- **Kiana 现在什么样**：Kiana 是 Rust workspace，Workflow 确定性刚补规范，无检查点 / 回滚，EventLog 是 JSONL，Reviewer 裁决只看改动文件清单。
- **建议改什么**：新增 `27-grok-build.md`，把 xai-workflow 的 journal（req_hash + 稠密 seq + Divergence fail-closed）作为 Kiana 确定性 workflow 的对照实现，hunk-tracker 的归因模型补强 Reviewer 证据，CoW worktree 作为 checkpoint 隔离参考。只取 workflow / journal / hunk / worktree / sandbox 形状，不引入其 provider、voice、dashboard、遥测与远程服务。
- **改哪份文档**：`docs/reference-agent-audit/` 新增 `27-grok-build.md`；`docs/company-os-reference-matrix.md` §2 / §3；`docs/company-os-platform-architecture.md`（恢复 / 确定性节）。

**S-3（重要）补审计：temporal-sdk-python、container-use、beads、graphiti / mem0、a2a、Archon-Knowledge、spec-kit / OpenSpec**

- **参考了谁 / 证据**：`reference/temporal-sdk-python/temporalio/worker/_replayer.py`；`reference/container-use/environment/state.go`、`mcpserver/tools.go`、`rules/agent.md`；`reference/beads/issueops/blockedstate.go`、`issueops/claimer.go`、`memoryops/memories.go`；`reference/graphiti/graphiti_core/edges.py:271-280`、`reference/mem0/mem0/memory/main.py`；`reference/a2a/specification/a2a.proto:187-210`；`reference/Archon-Knowledge/packages/workflows/src/dag-executor.ts`、`packages/isolation/src/backend-router.ts`；`reference/spec-kit/templates/*.md`、`reference/OpenSpec/src/core/artifact-graph/`。
- **Kiana 现在什么样**：这些能力在规范里都刚画了骨架或还是空白——durable replay、任务图、记忆时间维度、per-agent 隔离、协议状态机、DAG 工件校验。
- **建议改什么**：按能力补审计报告，只取形状不引入依赖：temporal 取本地 history-replay 语义（不引入服务端）；container-use 取环境状态机与「所有副作用经 environment」规则（不引入 Dagger）；beads 取 blocked 谓词与 claim 冲突类型（不引入 Dolt 后端）；graphiti / mem0 取时间 schema 与 op 生命周期（不引入 Neo4j / 向量库）；a2a 取 TaskState 映射与 artifact / part 形状（不做远程 transport）；Archon-Knowledge 取 DAG + 工件指针 + isolation 后端路由；spec-kit / OpenSpec 取工件模板链与 validate。
- **改哪份文档**：`docs/reference-agent-audit/` 新增对应条目；`docs/company-os-reference-matrix.md` §2 按能力插行；`reference/COMPANYOS-REFERENCES.md`。

**S-4（重要）刷新参考矩阵：从 26 项扩到覆盖现状**

- **参考了谁 / 证据**：`docs/company-os-reference-matrix.md:9-10` 仍写「对照 26 项目审计清单」「覆盖 24 个结构化审计项目」，`docs/reference-agent-audit/README.md:3` 也写 26 项；但 `reference/` 已有 72 个可见子目录（另有隐藏的 `.claude-flow`），70 个仓库带 `.git`、其中 47 个在 2026-08-25 后仍有提交，`reference/COMPANYOS-REFERENCES.md` 已列 Graphiti / Temporal / Mem0 等而矩阵未同步。
- **Kiana 现在什么样**：矩阵是「能力→参考项目」速查表，与审计 README 和 COMPANYOS-REFERENCES.md 三者脱节。
- **建议改什么**：在 §2 能力表按缺口插行（Workflow 确定性→grok-build / temporal；任务图→beads / claude-task-master；记忆时间→graphiti / mem0；隔离 / 检查点→container-use / gastown；协议→a2a；spec 工件→spec-kit / OpenSpec），§3 补小节并标注审计状态与最后核验日；同步 COMPANYOS-REFERENCES.md；`promptfoo-full` 的远程地址指向本仓，明确排除。**不能把「目录里有」当成「已审计」。**
- **改哪份文档**：`docs/company-os-reference-matrix.md` §2 / §3；`reference/COMPANYOS-REFERENCES.md`；`docs/reference-agent-audit/README.md` 覆盖状态表。

**S-5（常规）已核验仍有效的项目加「最后核验日 / 基准 commit」列**

- **参考了谁 / 证据**：roo-code / Roo-Code（最后提交 2026-05-15 b867ec914，审计日后 0 提交）、aider（2026-05-22 5dc9490bb，0 提交）、continue（2026-07-20 5522c6f44，0 提交），以及 claude-code-rust / gpt-pilot / autogen / ChatDev / MetaGPT / 12-factor-agents 同样 0 提交；crush（约 60 提交，多为 legal / docs / UI）、opencode（约 135 提交，集中在新增 console / stats 包）只有边缘漂移。
- **Kiana 现在什么样**：这 10 份审计写于 2026-08-25/26，此后对应仓库无新提交，结论仍有效；但 README 覆盖状态表没有「最后核验日」，读者无法区分「已核验仍有效」和「没再核验」。
- **建议改什么**：不必重审这 10 份；在 `docs/reference-agent-audit/README.md` 覆盖状态表加「最后核验日 / 基准 commit」列；crush / opencode 只核对 Kiana 实际引用的路径（crush 的 cancel / permission、opencode 的 event projector），不做全量重审。
- **改哪份文档**：`docs/reference-agent-audit/README.md` 覆盖状态表；`docs/company-os-reference-matrix.md` §3。

**S-6（常规）低优先补审计：superpowers、planning-with-files、claude-task-master、graphify、gastown**

- **参考了谁 / 证据**：`reference/superpowers/skills/*/SKILL.md` 与 `hooks/session-start`（渐进披露、会话启动注入）；`reference/planning-with-files/skills/planning-with-files/SKILL.md`（文件化计划 + attest）；`reference/claude-task-master/packages/tm-core/src/modules/dependencies/`（依赖图与 next-task）；`reference/graphify/graphify/extractors/rust.py`、`cache.py`（多语言代码图与缓存新鲜度）；`reference/gastown/internal/checkpoint/checkpoint.go`、`internal/estop/estop.go`（崩溃恢复与全局急停哨兵）。
- **Kiana 现在什么样**：skill 机制刚起步，无任务图 / 检查点，检索是朴素实现。
- **建议改什么**：补目录级或结构化审计，只取形状：superpowers 的按需技能索引（只读注入，不得成为执行路径）、planning-with-files 的 attest 校验映射到 gate、claude-task-master 的依赖建模、graphify 的 extractor 接口与 cache freshness、gastown 的 checkpoint 字段与 estop 熔断。gastown 的 mail / nudge / mayor 是多人 agent town，属于冻结的自由消息总线，只取 checkpoint / estop 两个机制。
- **改哪份文档**：`docs/reference-agent-audit/` 新增条目；`docs/company-os-reference-matrix.md` §2；`docs/features/10-context-search.md`。

## 六、被丢弃的建议及原因

### 6.1 违反硬约束，直接丢弃

| 原始建议 | 来自 | 违反的约束 |
|---|---|---|
| A2A push notification / streaming / SubscribeToTask 的传输层 | a2a `specification/a2a.proto:87-131` | token streaming、远程执行（冻结项）。只保留「对外声明 push_notifications=false / streaming=false」和 TaskState 映射 |
| A2A CancelTask 返回 CANCELED 终态 | a2a `a2a.proto:64-75`、`:186-208` | fail-open：会把「已请求取消」写成「已停止」。改用 `cancel_requested` / `result_unknown` |
| AutoGen handoff / group-chat / magentic、crewAI DelegateWorkTool、ADK transfer_to_agent_tool | autogen `_group_chat/_magentic.py`、crewAI `delegate_work_tool.py`、adk `tools/transfer_to_agent_tool.py` | 自由多 agent 消息总线 + 新增模型可见工具。委派只做 ControlPlane 操作 |
| cline Hub / agent-framework Foundry checkpoint / temporal server / container-use Dagger / gastown town-mail / grok-build remote 面 | cline `apps/cline-hub/`、agent-framework `_workflows/_checkpoint.py`、container-use `service.go`、gastown `internal/mail/` | 企业托管 / 远程执行 / 第二运行时。只取本地形状 |
| OpenHands Planner / Canvas Extensions 云托管面、codex Windows sandbox provisioning | OpenHands `routes/planner-tab.tsx`、codex `windows-sandbox-rs/` | 远程 / 平台化。只写审计边界，不实现 |
| roo 自动选择倒计时自动提交审批、aider 默认 auto-commit、mini-swe confirm 当作安全边界 | roo `FollowUpSuggest.tsx`、aider `args.py:396-400`、mini-swe `interactive.py` | 代用户决策 / 把 UX 当授权。自动提交绝不用于审批类 ask |
| agent-framework 动态 agent 对话式编排、Archon 自由 spawn orchestrator、claude-task-master 模型驱动规划 | agent-framework `_workflows/_runner.py`、Archon-Knowledge `orchestrator.ts` | 编排权交给模型 / 第二执行路径。图只能由 ControlPlane 拥有 |
| 任何「新增第六个工具 / 把 skill 当工具 / 让模型调用 delegate」 | 多个框架 | 五工具面锁死 |

### 6.2 合并去向（去重后并入哪一条）

| 原始发现 | 去向 |
|---|---|
| 批量 / 部分批准、自动批准类别开关、自动批准预算闸、Guardian 自动审查器 | 并入 I-2 / A-4 / A-6；自动批准只能是「policy 明确允许的会话级放行」，审查器只建议 / 拒绝，ControlPlane 仍是最终授权者 |
| 审批请求-响应配对与类型校验 | 并入 I-1 |
| 跨会话待审批提示 | 并入 I-5 |
| checkpoint 预览 / 只恢复文件 vs 连同对话、编辑级 undo、diff 收据 | 并入 A-10 |
| token / 成本 / 上下文占用呈现、shell started/output/exited、delta 批处理、keyset 分页、CLI 终局兜底、progress token | 并入 I-6 / I-7 / I-9；对外表述仍是「非 streaming 进度反馈」 |
| 会话 load / fork 并发去重、会话状态带作用域 delta | 并入 A-1 / A-2 |
| heartbeat / lease、convoy、父子 packet 上卷、状态投影行协议、路径不相交垂直切片 | 并入 A-3 / F-7 |
| 混合检索 BM25 + 向量 + RRF、attribution guard、近重复合并、记忆负向测试 | 并入 F-3；检索分数只排序不授权 |
| catalog 信任栈、managed / vendored 安装形态、skill lint、渐进披露、spec delta 合并 | 并入 F-2 / F-7 / S-6 |
| continue-as-new、run 启动冲突策略、signal/query/update 分类、确定性输入、版本迁移 | 并入 A-1 / A-2 / A-7 / A-9 |
| 沙箱失败升级重试、组合式权限模型、patch 独立安全评估 | 并入 A-5 / A-6；deny 条目绝不因去沙箱执行被绕过 |
| spec-kit 宪法检查、跨产物 analyze、validate CLI | 并入 F-7 |
| mem0 模型驱动的 add/update/delete | 保留 op 生命周期，但来源必须服务端派生（并入 F-1 / F-3） |

## 七、需要你拍板的问题

1. **auto-approve 到底保不保留？** 现在只有 `--approve-local-write` 对本地写自动批准。UI/UX §5.3.2 已经把它列为开放决策：保留的话，风险上限钉在哪一档？是否必须在事件和收据里显式披露「这里发生过自动批准」？
2. **审批决定词汇首发取哪几个？** 「批准这一次 / 本会话 / 持久 prefix 规则 / 拒绝并继续 / 拒绝并中止」五个都要，还是先上「这一次 + 拒绝并继续」两个？「本会话批准」和「持久规则」都涉及存储与撤销，工作量差很多。
3. **跨进程恢复的默认行为是什么？** 重启后是默认「暂停、等人显式恢复」，还是自动重建待审批列表并继续？我倾向默认暂停（fail-closed），但这会改变现在「重启后单子还挂着但活续不上」的体验。
4. **检查点 / 回滚首发做到哪一层？** 只做会话 / 状态层（便宜、风险低），还是也做文件层（需要 shadow git、事务化恢复、危险路径防护，工作量大）？文件层要不要沿用 roo 的 shadow git 方案？
5. **任务图是不是首发做？** 依赖边先用 `WorkPacket.dependencies` 显式字段，还是从 packet 文本解析？`ready_packets` 和 `PathLock` 的关系要不要在规范里写死「规划期检查 + 运行期兜底」？
6. **记忆 candidate 默认不可检索，会不会误伤？** 现有 Builder scratch 写入如果也走 `memory.write`，升级后可能在测试里变不可见。要不要区分 instance-scratch 与持久层，各用各的默认可见性？
7. **重审顺序怎么排？** 先重审 codex / goose / adk-python / deepseek-harness / agent-framework（旧审计维护），还是先补 grok-build 新审计（可能直接影响 Workflow / 检查点设计）？两者都要不少时间。
8. **参考矩阵要不要按「最后核验日」长期维护？** 现在 72 个目录对 26 项审计，如果每次参考项目更新都重审，成本很高；是否接受「结构化审计 + 最后核验日 + 只核对被引用路径」这套轻量维护方式？

## 相关文档

- `docs/company-os-reference-matrix.md`：能力→参考项目速查表，本文的建议应回填到这里。
- `docs/company-os-ui-ux.md` §5.3 / §5.4 / §6.5 / §7.2 / §10 / §11：交互建议的规范落点。
- `docs/features/05-approvals.md`、`09-session-lifecycle.md`、`10-context-search.md`：本文「Kiana 现在什么样」的主要事实来源。
- `docs/company-os-platform-architecture.md`、`company-os-design.md`、`company-os-domain-contracts.md`、`company-os-security-constitution.md`：架构建议的规范落点。
- `docs/reference-agent-audit/README.md`：审计覆盖状态与最后核验日应更新处。
- `CURRENT_STATUS.md`：判断「现在是什么」的唯一当前事实来源，本文不改变它。
