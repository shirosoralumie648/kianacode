# 功能走读（现实层）

这套文档是**现实层**：每篇讲一个功能的代码真实现状——哪个函数、什么行为、哪些边界是硬拒的。它和另外两层分工明确：`company-os-*` 规范文档写"要成为什么"，`CURRENT_STATUS.md` 写"证明了什么、按什么证据"，这一层写"代码现在长什么样"。三层说法冲突时，按 `docs/README.md` §3 的权威顺序处理：事实压过规范。

每篇的固定结构：这个功能是干什么的 → 现在能干什么 / 不能干什么（每条带代码出处）→ 代码怎么跑（走读）→ 关键概念速查 → 现在最明显的短板。

| 编号 | 标题 | 一句话 | 什么时候读 |
|---|---|---|---|
| 01 | [模型执行与 provider 选择](01-model-execution.md) | Kiana 怎么决定"谁在动脑子"：cassette 脚本永远优先，其次四家 provider，没配 API key 就明确失败 | 想知道模型从哪来、为什么必须配 key 或脚本 |
| 02 | [工具调用与授权脊柱](02-tool-call-spine.md) | 每次工具调用都要走"CapabilityRequest → 审批 → broker 派发 → 执行 → 事件回执"这条链，模型碰不到你的电脑 | 想理解整个产品最重要的安全链条 |
| 03 | [五个模型可见工具](03-five-tools.md) | shell / apply_patch / mcp / memory.search / memory.write 五只"手"各长什么样：怎么调、过哪些检查、哪里硬拒；工具面锁死为五个是有意设计 | 想知道模型每只"手"的真实边界、为什么不许加第六个工具 |
| 04 | [信任、沙箱与路径安全](04-trust-sandbox-path.md) | trust 决定能不能进门，sandbox 决定能碰哪里，path lock 防两个任务撞车 | 想知道为什么没 trust 连只读都被拒、写盘要同时满足哪些条件 |
| 05 | [审批与人工确认](05-approvals.md) | 有风险的动作按暂停、开审批单，等你本人点头才继续；单子带指纹、nonce 和 5 分钟有效期 | 想知道什么时候会被打断、批准为什么只能用一次 |
| 06 | [事件账本与回执](06-eventlog-receipts.md) | 每件事记进只能追加的 JSONL 账本；回执是从账里算出来的，不是新记的 | 想知道"AI 到底干了什么"去哪查、能不能信 |
| 07 | [角色、部门与任务流程](07-roles-departments.md) | 五部门六角色：规划会出工单、Builder 开全新会话施工、Reviewer 验收、Closer 结案，隔离规则全是硬拒 | 想理解多角色流水线和"Builder 不能列席规划会"这类边界 |
| 08 | [三个界面](08-surfaces.md) | `kiana run` / `kiana` / `kiana web` 三个入口共用同一个 DaemonHost，规则只有一套 | 想知道三个入口有什么区别（答案：几乎没有） |
| 09 | [一次任务的完整一生](09-session-lifecycle.md) | run 从 Accepted 到 Completed/Failed/Cancelled 的状态流转、continue/cancel 的语义和停不干净的角落、compaction 和 inbox | 想知道任务卡住、取消、压缩历史时到底发生了什么 |
| 10 | [上下文与搜索](10-context-search.md) | repo map、关键词/向量搜索、context pack 全在本地、全只读；模型搜代码靠 shell 跑 rg | 想给 AI 喂背景资料，或自己先摸一遍项目 |

**推荐阅读顺序**：08（界面）→ 09（任务一生）→ 02（授权脊柱）→ 01（模型）→ 03（五个工具）→ 04 → 05 → 06 → 07 → 10。
