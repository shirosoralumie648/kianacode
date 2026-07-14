# Kiana Domain Pitfalls

**Project:** Kiana
**Researched:** 2026-07-14
**Mode:** Brownfield project-risk research
**Overall confidence:** HIGH for current-repository risks; MEDIUM for future hosted-scale risks

## Executive Warning

Kiana 最可能失败的方式不是“少几个功能”，而是在扩展三条产品线和全部产品入口时形成多套事实来源、权限语义和完成定义。当前仓库已经具备大量 runtime、workflow、evidence、trust、remote 和 release 基础；如果新表面继续直接写 session/task/workflow 文件，或把模块、命令和 mock 测试当成完成证据，项目会在看似快速扩张后进入无法恢复的状态分裂。

路线图必须先冻结 contract、统一状态权威、抽取 policy/runtime 边界，再扩展能力包和产品入口。Local、Cloud 和 Enterprise 可以使用不同存储适配器，但必须共享同一事件、策略、任务、证据和恢复语义。

## Critical Pitfalls

### 1. Session、Task、Workflow 与 Memory 形成多个可写权威

**What goes wrong:** SDK session JSON/JSONL、task list、workflow EventLog/state、memory、app-server 数据库和未来云端行都能独立修改同一任务状态。

**Why it happens:** 当前持久化路径分散；新增 UI、云端或能力包时，直接写最方便的文件或表比接入统一 port 更快。

**Consequences:** 跨入口看到不同状态；恢复后任务倒退或重复；审计和证据无法证明哪条记录真实；同步采用 last-write-wins 后丢失事实。

**Prevention:** Session 与 Workflow 使用追加式事件作为事实来源；task board、SQLite、PostgreSQL、搜索索引和 UI store 都是可重建 projection。所有跨 aggregate 修改使用 correlation ID、idempotency key 和显式 command acknowledgement。

**Detection:** 同一状态字段在多个模块直接写入；云端表和本地 EventLog 没有事件 ID/hash 对应；测试通过删除 projection 后无法完整重建；任务状态从 Assistant prose 推断。

**Recovery:** 冻结写入，选择可信 EventLog 为基线，生成差异报告，重建 projection；无法证明顺序或完整性时进入 `blocked`，不得自动合并。

**Phase owner:** Contract Baseline、State Authority、Reliable Execution Unification。

### 2. Wire Contract 与 Rust/Client/Schema 漂移

**What goes wrong:** CLI JSON、`RuntimeEvent`、MCP、app-server、remote/bridge、Desktop/IDE 生成类型和 `docs/schemas/` 对同一字段产生不同定义。

**Why it happens:** 当前有大量手写 schema；每个产品入口独立迭代，mega-file 路由使局部变更影响面不透明。

**Consequences:** 老 session 无法回放；客户端静默忽略安全字段；权限和 completion 状态在不同入口含义不同；升级破坏自动化。

**Prevention:** 先冻结 typed IDs、terminal states、error taxonomy、event durability 和 versioning；新 contract 从 Rust/schema 单一来源生成客户端类型；已发布 v1 schema 只能通过兼容迁移演进。

**Detection:** 同一 JSON shape 在多个 crate/TypeScript 文件手写；新增字段没有 schema fixture；schema smoke 与真实二进制输出不使用同一样例；未知 enum 被默认映射为 success。

**Recovery:** 保留旧 adapter，回滚不兼容 emitter，增加 golden replay/migration；不能自动迁移的数据必须提供 export/diagnostic，而不是丢弃。

**Phase owner:** Contract Baseline，且后续每个 surface phase 都必须重跑 contract gate。

### 3. 在 Mega-files 中继续堆叠跨领域行为

**What goes wrong:** 新功能继续进入超大的 `cli.rs`、`runner.rs`、`tasks.rs`、`plugin.rs` 或 `agent.rs`，入口、状态、策略和领域逻辑无法隔离。

**Why it happens:** 当前路由和运行循环集中，直接增加分支能最快展示功能。

**Consequences:** 小改动产生全局回归；无法为 Coding/Research/Daily 独立验证；后续抽取 runtime/policy/state 时需要大爆炸重写。

**Prevention:** 先建立 `kiana-state`、`kiana-policy`、`kiana-runtime` ports；入口仅做解析和呈现；领域状态进入 owning crate；每次只抽取一个可验证 seam。

**Detection:** 新命令同时修改入口、provider loop、权限和持久化；文件行数持续上升；测试只能从整个 CLI 验证内部规则；低层 crate 开始依赖 `kiana-entrypoints`。

**Recovery:** 停止功能扩张，补 contract tests，按 route/domain 单元逐步抽取；禁止以 greenfield rewrite 规避兼容责任。

**Phase owner:** Policy/Runtime extraction 之前及期间。

### 4. 把“多 Provider”降成最低共同能力或伪装成完全等价

**What goes wrong:** Anthropic、OpenAI、Gemini、OpenRouter、OpenAI-compatible 和本地模型被塞进一个布尔能力集合；不支持 tool use、vision、structured output、long context 或 reasoning 的 Provider 被静默模拟。

**Why it happens:** 产品要求首版多供应商，界面又希望隐藏差异。

**Consequences:** 工具调用丢失、结构化结果不可靠、费用/上下文超限、工作流在特定模型上无法恢复；“Claude Code 功能覆盖”只在一个 Provider 上真实成立。

**Prevention:** Provider catalog 保存带来源和时间的 capability snapshot；每个 turn 冻结模型能力；路由、显式降级、命名 emulation 或拒绝；黄金任务按 Provider capability class 执行。

**Detection:** Provider-specific 分支散落在 UI/commands；所有模型展示相同能力；测试只有 FakeProvider；失败后自动切换模型但不记录语义变化。

**Recovery:** 将受影响运行标记为不可验证，保留原始事件和模型快照；使用兼容 Provider 重新计划，不把不同模型输出直接拼成原任务完成证据。

**Phase owner:** Contract Baseline、Runtime Extraction、Provider/Eval gates。

### 5. 外部写入超时后盲目重试

**What goes wrong:** 邮件、消息、发布、付款、权限变更、云端提交或桌面操作已发生，但连接在回执前中断；Agent 把超时当失败并再次执行。

**Why it happens:** 普通工具错误模型只有 success/error，没有 attempted、acknowledged 和 `result_unknown`。

**Consequences:** 重复付款/发送/发布、数据删除、权限扩大；审计无法解释重复动作；用户不再信任自治模式。

**Prevention:** 所有外部副作用使用 idempotency key、标准化目标、持久化审批、attempt event 和 receipt；响应丢失进入 `result_unknown`，只有查询证明未发生或人工裁决后才能重试。

**Detection:** connector 没有幂等和查询接口；timeout handler 直接重放；测试只覆盖确定成功/失败；工具结果不包含外部 object/event ID。

**Recovery:** 自动执行立即停止；查询外部系统并记录 reconciliation evidence；无法确认时保持 `result_unknown` 并请求人工处理。

**Phase owner:** Reliable Execution Unification、Daily Pack、Cloud/Enterprise。

### 6. 自主权档位绕过 ProjectTrust、组织策略或硬审批

**What goes wrong:** “自治”被实现成全局 allow；项目内 skill/plugin/hook/MCP 能修改自身授权；低层用户配置覆盖企业 deny；remote worker 获得环境级凭据。

**Why it happens:** 权限判断分散在 tools、runner、skills、hooks、remote 和 UI；体验压力促使新增快捷绕过。

**Consequences:** 供应链执行、凭据泄漏、任意网络/文件访问、跨租户动作和不可审计修改。

**Prevention:** 纯函数式 `kiana-policy` 解析 versioned snapshot；组织 hard deny 单调优先；ProjectTrust 存在项目外；worker 使用短期 scoped capability；每个入口通过同一 policy gate。

**Detection:** `--yes`/auto mode 可跳过 trust；项目本地文件能写 authoritative trust；MCP/app-server 直接调用 tool；只有允许路径测试，没有 deny/bypass tests。

**Recovery:** 吊销 capability/token，隔离项目和插件，保留审计证据，轮换凭据；受影响 workflow 标记 integrity blocked 并重新验证。

**Phase owner:** Policy And Typed Context；所有 pack/surface 的安全门禁。

### 7. 多 Agent 没有租约、隔离和集成门禁

**What goes wrong:** 多 worker 同时修改相同文件/资源；失联 worker 被重派后又返回；子 Agent 直接修改 parent state；“完成”只来自子 Agent 文本。

**Why it happens:** 并行数量被当作能力指标，WorkPacket 和 ResultPacket 只做描述而未成为强制协议。

**Consequences:** 丢失用户改动、重复外部副作用、合并基线错误、证据串线、成本失控。

**Prevention:** 启动前持久化 WorkPacket；文件使用 worktree/snapshot 和 path lock，非代码资源使用 concurrency key；lease/capability 有期限；late result 不自动集成；parent 完成前重跑全局 verification。

**Detection:** worker 能写 parent workflow；任务没有 allowed paths/budget/acceptance schema；lease expiry 后结果仍自动合并；并发测试只使用互不相交 happy path。

**Recovery:** 冻结集成队列，保留所有 ResultPacket 和 baseline hash，重新验证/重放到新隔离环境；有副作用的 late result 进入 reconciliation。

**Phase owner:** Reliable Execution Unification、各 Domain Pack。

### 8. Evidence Theater：本地文件或测试数量冒充产品完成

**What goes wrong:** 命令、模块、schema、页面、mock response 或大量通过的单元测试被标记为“功能完整”；本地 proof 被当成签名、渠道、托管、entitlement 或客户验收证据。

**Why it happens:** 代码存在容易统计，外部发布证明成本高；旧文档和 README 可能使用乐观完成语言。

**Consequences:** 1.0 在真实安装、升级、故障、用户旅程或商业环境中失败；发布声明不可审计。

**Prevention:** 每个 requirement/reference capability 绑定实现、contract test、真实黄金任务、target-environment proof 和用户价值证据。保留 `local_blocking` 与 `external_blocking` 区分，release smoke 是最低门槛而非最终证明。

**Detection:** blocker report 仍有 blocking 条目却宣称 production-ready；使用 `--skip` 的 smoke 作为最终证据；测试 fixture 预写成功 JSON；没有目标平台安装/回滚和真实外部回执。

**Recovery:** 撤回完成标签，重新生成 blocker/proof inventory；将缺口映射到 owner、环境、命令和验收产物，不制造本地替代证明。

**Phase owner:** 从 Phase 0 tracking 开始，Release Proof 最终负责。

### 9. Research Pack 污染证据链

**What goes wrong:** 模型生成不存在的 DOI/引用、把摘要当全文、混淆训练/验证/测试、修改实验结果或统计口径、把未运行代码描述为复现成功。

**Why it happens:** 自然语言输出流畅，检索来源质量不一；论文写作、实验和引用由不同 Agent 处理却缺少统一 provenance。

**Consequences:** 学术不端、不可复现、错误结论、投稿风险和用户决策损害。

**Prevention:** 每个 claim 绑定 source/artifact/hash；下载与解析记录 final URL、许可和版本；实验记录环境、seed、数据 split、代码 commit 和原始指标；paper writer 只能引用 evidence graph 中可验证节点。

**Detection:** 引用没有 DOI/URL/本地 artifact；统计表只有渲染结果没有原始数据；Agent 可编辑 evidence source；测试只检查 Markdown 格式。

**Recovery:** 隔离受污染结论，重新从原始来源/实验构建 evidence graph；所有依赖 claim 自动降级为 unverified，禁止进入最终稿。

**Phase owner:** Research Pack vertical slice、Research eval/release gates。

### 10. Desktop、Browser 与 Daily Automation 把便利接口变成特权后门

**What goes wrong:** Tauri invoke、浏览器扩展、native host、screen/input、clipboard、OAuth connector 或 RPA 直接获得 Core/OS 广泛权限；第三方网页可触发 privileged action。

**Why it happens:** 桌面集成跨越 Web 与 native 边界，演示场景偏向“能控制一切”。

**Consequences:** 任意命令/文件访问、prompt injection 驱动外部写入、凭据和隐私泄漏、不可恢复的桌面动作。

**Prevention:** 明确 capability manifest、origin/target binding、最小权限 command、preview/approval/receipt；不暴露 generic privileged invoke；screen/clipboard/notification 数据按敏感级别处理。

**Detection:** Web 内容能构造 native command；connector token 进入模型上下文；自动化没有目标窗口/资源 identity；没有 result-unknown 测试。

**Recovery:** 关闭 connector/capability，撤销 token，保存审计与截图/回执，核对外部状态；高风险动作需人工恢复。

**Phase owner:** Surface And Local Continuity、Daily Pack、安全审查。

### 11. Local-first 被 Cloud 或 Enterprise 控制面侵蚀

**What goes wrong:** 本地功能开始要求登录、在线 entitlement、云数据库或控制面；同步把云端行变成最终权威；企业策略和个人配置优先级不一致。

**Why it happens:** 商业功能先在 hosted path 实现，团队协作推动服务端成为默认。

**Consequences:** 离线不可用、供应商锁定、数据主权承诺失效；本地/云任务恢复语义分叉。

**Prevention:** Local runtime/State/Policy ports 完整实现；Cloud 是显式 adapter；同步交换加密 versioned events/artifacts/tombstones，不同步 secrets；云断开时本地继续并有 bounded queue/backpressure。

**Detection:** local tests 需要 account/token；HTTP handler 直接拥有领域逻辑；cloud row 与 local event 冲突用 last-write-wins；本地导出无法离开官方服务恢复。

**Recovery:** 回退到本地 authority，停止同步，导出冲突日志并人工/规则化合并；云端不可验证事实不能覆盖本地完整事件。

**Phase owner:** RuntimeHost、Cloud/Enterprise、Sync gates。

### 12. Tenant Isolation 只存在于 API 层

**What goes wrong:** service handler 检查 tenant，但 repository 接受裸 aggregate ID；object store、queue、cache、metrics 或 vault 没有 tenant scope；worker 持有跨租户凭据。

**Why it happens:** 本地单用户模型直接扩展到 hosted mode，测试只覆盖正常租户。

**Consequences:** 跨客户数据读取/修改、证据和 artifact 泄漏、企业合规失败。

**Prevention:** 所有 hosted key/query 使用 `(tenant_id, workspace_id, aggregate_kind, aggregate_id)`；tenant 来自认证 context；数据库 RLS + repository filter；per-tenant keys、quotas 和 negative tests。

**Detection:** repository method 接受裸 ID；bucket path/queue subject 未含 tenant；管理员 token 被 worker 复用；stream/list endpoint 缺少跨租户拒绝测试。

**Recovery:** 停止受影响服务、轮换 keys/token、审计访问范围并通知；修复后重建 tenant-scoped indexes 和 proof。

**Phase owner:** Cloud Control Plane、Enterprise Isolation。

### 13. Reference 覆盖演变成许可证和 Stub 污染

**What goes wrong:** 为满足“全部参考”直接复制专有/不兼容源码，或把 archived、maintenance-mode、mock/stub 项目当成生产实现；旧文档分类替代 live repo 证据。

**Why it happens:** repository 名称和 feature list 比逐项 license/implementation audit 更容易使用。

**Consequences:** 法律风险、不可靠依赖、错误架构迁移、能力矩阵虚假完成。

**Prevention:** 每项能力记录 snapshot、source path、license、`Adopt / Adapt / Reject`、Kiana owner、test、evidence 和风险；Claude 产品只做公开行为层独立实现；live source 优先于旧摘要。

**Detection:** 没有 license record 就出现复制代码；reference module 只有 mock service；矩阵只列项目名没有具体 mechanism/source；分类与当前仓库内容不符。

**Recovery:** 隔离来源不明代码，重做 provenance/license scan，替换为 Kiana-owned contract 实现；撤销无法证明的 reference parity 标记。

**Phase owner:** Reference Matrix/Tracking，且每个采用能力的 phase 必须复核。

### 14. 跨平台与供应链只在开发机上成立

**What goes wrong:** Rust/Node/Actions/cargo tools 使用浮动版本；terminal/WebSocket 依赖多版本分裂；Linux/macOS/WSL 的沙箱、Keychain、路径和 updater 行为没有真实安装证明。

**Why it happens:** 本地 Cargo tests 通过掩盖 packaged artifact、native dependency、签名和离线部署差异。

**Consequences:** 不可复现构建、二进制膨胀、协议差异、安全更新失败、企业离线安装不可用。

**Prevention:** 固定 Rust/Node/actions/tool versions 和 lockfiles；统一共享依赖；每种 delivery format 运行 build/install/upgrade/rollback/uninstall；生成 SBOM/provenance/signature；WSL 明确 secret backend 和 path/network contract。

**Detection:** CI 使用 floating `stable`/tag-only action；`cargo tree -d` 有未解释重复；release smoke 跳过 build/package；macOS notarization/WSL secret 状态没有 proof。

**Recovery:** 停止发布，重建受控工具链和 artifact，比较 hash/SBOM，撤销不可信渠道产物并重新签名。

**Phase owner:** Stack Foundation、Surface Packaging、Release Proof。

## Moderate Pitfalls

### Premature Microservices

**What goes wrong:** domain contracts 未稳定就拆分多个服务，引入分布式事务、重复 retry 和部署复杂度。

**Prevention:** 先采用 modular control plane + isolated workers；只有测量到吞吐、隔离或团队所有权压力后才拆服务。

### Database Projection 反客为主

**What goes wrong:** SQLite/PostgreSQL 查询方便，逐渐绕过 EventLog 写入，形成新的事实来源。

**Prevention:** repository API 区分 event append 与 projection rebuild；任何 projected row 保留 source event identity/hash；数据库损坏测试必须能从日志恢复。

### UI Store 成为 Workflow State

**What goes wrong:** React/Tauri/IDE store 在重连后覆盖 Core 状态，或不同 UI 使用不同 terminal status。

**Prevention:** UI 使用 cursor/snapshot/event reducer；未知事件触发 `resync_required`；所有 command 返回 idempotent acknowledgement。

### Observability 泄漏 Prompt、文件和凭据

**What goes wrong:** OpenTelemetry、debug bundle 或 audit log 捕获用户内容、tokens 和 secrets。

**Prevention:** 结构化 metadata 与 redaction 默认；本地 telemetry 关闭；诊断导出在用户确认前展示内容清单；高基数/敏感字段禁止进入 metrics。

### 在真实 Retrieval Eval 前引入 Vector Service

**What goes wrong:** 把 deterministic fixture 当成 RAG 证明，引入新数据库和同步路径，却没有提高任务完成率。

**Prevention:** SQLite FTS/现有 deterministic search 优先；只有版本化 recall/latency/task-success eval 证明收益后才启用向量扩展。

### 串行测试掩盖全局状态设计缺陷

**What goes wrong:** `--test-threads=1` 让 release gate 稳定，却隐藏 cwd/env/global cache 在真实并发 runtime 中的竞争。

**Prevention:** release gate 可继续串行，但逐步把测试改成 process/temp-root/explicit context 隔离，并增加并发 stress/fault tests；retry 不能掩盖 race。

### Dirty Worktree 被误当成稳定 Baseline

**What goes wrong:** 规划或验证基于旧 commit，而大量未提交实现、schema 和 scripts 改变真实行为。

**Prevention:** 每个 phase intake 记录 commit、dirty paths、测试命令和 ownership；不覆盖用户修改；集成前对 live tree 重新运行 focused + workspace/release gates。

## Minor Pitfalls

### 术语和 ID 不统一

Session、thread、turn、workflow、task、worker、agent 和 packet 如果没有 typed ID，会在日志、API 和 UI 中混用。Contract phase 必须冻结 glossary 和 ID wrappers。

### 文档更新晚于 Schema

CLI/API 改动若只更新 Rust 或 schema，会让 client、admin 和 release guides 失真。Contract diff 应自动生成需要更新的文档清单。

### 成本、Token 和 Artifact 无预算

长任务和多 Agent 如果只设逻辑目标，没有 token/time/storage/tool budget，会在云端形成不可控费用。预算耗尽必须是结构化 blocker，而不是被当成普通模型错误。

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Required Mitigation / Exit Signal |
|-------------|----------------|-----------------------------------|
| Reference Matrix And Tracking | 旧分类、stub 或许可证不明仍被计为覆盖 | 38/38 live rows 含 source、license、decision、owner、test、evidence；Reject 有理由 |
| Contract Baseline | schema、terminal states、error/event durability 不一致 | Golden replay/serialization 覆盖 CLI/SDK/MCP/app-server/remote/bridge；兼容 adapter 通过 |
| State Authority | dual-write 长期存在并产生 split brain | Event append 是 commit point；projection 可删除重建；迁移/回滚/export proof 通过 |
| Policy And Typed Context | autonomy/remote/enterprise 绕过 deny | 同一 policy evaluator 覆盖所有入口；negative bypass、deny precedence、trust tests 通过 |
| Runtime Extraction | 大爆炸重写或 provider 行为漂移 | 每个 slice 保留公开 API wrapper；Fake + real fixture parity、cancellation/retry/event tests 通过 |
| Reliable Execution | 外部 timeout 重放、多 Agent late result 集成 | `result_unknown`、idempotency、lease expiry、late result、whole-run verification fault tests 通过 |
| Coding Pack | Claude 功能清单替代真实仓库任务 | 版本冻结的功能矩阵 + 多语言真实 repo golden journeys + diff/test/review evidence |
| Research Pack | fabricated citation/data 或不可复现实验 | Claim-to-source coverage、dataset split、environment/seed/commit、reproduction gates 通过 |
| Daily Pack | Desktop/browser/connector 特权边界失控 | capability/origin/target/approval/receipt contract 与 prompt-injection/timeout tests 通过 |
| Surfaces And Local Continuity | CLI/IDE/Desktop/Web 各自持有状态 | RuntimeHost cursor/snapshot/event parity；断线重连、跨入口审批和 daemon restart journeys 通过 |
| Cloud Control Plane | cloud 成为本地依赖，outbox/worker 形成第二权威 | 离线 local journey、transactional outbox、scoped worker lease、cloud outage/backpressure proof |
| Enterprise Isolation | 只在 handler 做 tenant check | repository + RLS + object/queue/vault scope；每个 endpoint 的 cross-tenant negative tests |
| Packaging And Supply Chain | 本地 test 代替真实 artifact lifecycle | pinned toolchain/actions、SBOM/provenance/signature、Linux/macOS/WSL install/upgrade/rollback proof |
| Release Proof | local blocker 为零就宣布 1.0 | `local_blocking=0` 与 `external_blocking=0` 均有真实目标环境证据；无 skip、无 placeholder |

## Early Warning Dashboard

在每个 phase review 中至少检查以下信号：

- 新增了直接写 session/task/workflow/state/database 的路径；
- 新 surface 或 pack 发明了自己的事件、权限或 completion enum；
- Provider capability 未记录来源/时间，或发生 silent fallback；
- 外部写入没有 idempotency key、receipt 或 `result_unknown`；
- Worker 没有 lease、scope、baseline hash 或 ResultPacket；
- Research claim 没有 source/artifact/hash；
- Hosted repository/query 接受裸 aggregate ID；
- Reference 能力没有 license/decision/source path；
- release 命令使用 skip、fixture success 或本地替代外部 proof；
- 浮动工具链、重复依赖或未签名 artifact 进入发布路径；
- README/roadmap 的完成语言领先于 live blocker 和 target evidence。

任一 critical signal 未处置时，对应 phase 不得标记 complete。

## Sources

### Primary Repository Evidence (HIGH)

- `.planning/PROJECT.md`
- `docs/superpowers/specs/2026-07-14-kiana-complete-ai-agent-product-design.md`
- `.planning/codebase/CONCERNS.md`
- `.planning/codebase/TESTING.md`
- `.planning/codebase/ARCHITECTURE.md`
- `.planning/research/STACK.md`
- `.planning/research/FEATURES.md`
- `.planning/research/ARCHITECTURE.md`
- `docs/reference-migration-roadmap.md`
- `docs/reference-feature-matrix.md`
- `scripts/commercial-release-blockers-report.sh`
- `scripts/release-smoke.sh`

### Confidence Limits (MEDIUM)

- Cloud scale、multi-region、hosted queue 和 enterprise operations 仍是目标架构研究，必须在对应 phase 通过容量测试、故障注入和真实部署证据验证。
- Claude Code 与 Claude Desktop 是动态专有产品；功能覆盖必须冻结公开行为版本并持续记录差异，不能依赖训练记忆或非公开实现假设。

---
*Pitfalls research completed: 2026-07-14; main-agent fallback after the generic researcher failed to materialize its file.*
