# Kiana 功能版图研究

**领域：** 本地优先、Open Core、跨 Coding / Academic Research / Daily Work 的完整 AI Agent 平台
**研究日期：** 2026-07-14
**范围：** Kiana Core、三个能力包、全部产品入口、官方云、企业自托管，以及 38 个本地 reference 目录
**置信度：** Kiana 已批准范围与当前仓库事实 HIGH；reference 能力归因 MEDIUM-HIGH；无实时外部检索条件下的泛市场判断 MEDIUM

## 研究结论

1. Kiana 1.0 不是 Coding CLI 的最小版本。已批准的 Coding、Research、Daily、全部入口、官方云和企业自托管都属于同一个 1.0 门禁；Alpha/Beta 只用于按依赖顺序验证垂直切片，不能用于静默缩小正式范围。
2. 真正的依赖主线是统一契约先于产品外壳：runtime / session / provider / tool / policy → workflow / evidence / recovery / bounded multi-agent → 三个能力包 → 多端连续体验 → cloud / enterprise → 全量发布证明。
3. Coding 的表格基线应是“冻结日期的 Claude Code 公开行为清单 + 每项真实旅程证据”，不是命令名兼容或模块数量。当前本地材料明确覆盖 IDE、Desktop、Web/Cloud、MCP、subagent、plugins、skills、hooks、agent teams、checkpoints、Chrome、Git/GitHub Actions、SDK、voice 等类别。
4. Kiana 的主差异化不是更多按钮，而是能用持久状态、证据账本、verifier、结果未知状态和可恢复执行证明长任务确实完成。
5. 38 个 reference 目录提供机制和风险边界，不提供完成证明。许可证未知、非商业、AGPL、专有、归档、维护模式或 stub 密集的参考只能做行为参考或选择性独立实现。

## 分类与证据规则

| 标签 | 含义 | REQUIREMENTS.md 使用方式 |
| --- | --- | --- |
| Table stake | 用户合理预期 1.0 必须具备；缺失即产品不完整 | 转为 MUST requirement，并绑定至少一个端到端旅程 |
| Differentiator | 用户会主动选择 Kiana 的能力；多数也是已批准 1.0 核心价值 | 转为 MUST 或关键成功指标，不作为可选装饰 |
| Post-1.0 | 明确不影响已批准 1.0 完整性的扩展 | 进入后续 milestone，不得替代当前 1.0 项 |
| Anti-feature | 即使看似有吸引力也不得实现的行为 | 转为 NOT / safety / governance requirement |
| 已验证基础 | 当前 matrix 有代码、schema、测试或 smoke 证据 | 只能说明底层切片存在，仍需对应 1.0 旅程验收 |
| 局部基础 | 有部分契约或入口，但有明确 remaining risk | 不得写成 complete；requirement 必须保留缺口 |
| 目标能力 | 已批准但当前材料没有足够实现证据 | 作为新增 requirement，不推断已有实现 |

所有 feature ID 都是路线图和 REQUIREMENTS.md 的稳定引用。复杂度是达到公开发行质量的整体复杂度，不是仅创建类型、命令或页面的代码量。

## Table Stakes

### Kiana Core

| ID | 可观察的 1.0 功能 | 为什么是基本盘 | 目标入口 / 形态 | 依赖 | 复杂度 | 当前证据边界 |
| --- | --- | --- | --- | --- | --- | --- |
| CORE-01 | 所有 turn、stream delta、tool call/result、permission、error、usage 和 terminal result 都输出版本化 typed RuntimeEvent；每个入口能重放同一事件序列 | 没有共同协议就无法跨 CLI、IDE、Desktop、Web 恢复同一任务 | 全部入口、Local、Cloud、Enterprise | 无，最底层契约 | HIGH | typed events 与多适配器已有验证基础；新领域事件仍需纳入同一 schema |
| CORE-02 | session 可 create/list/resume/fork/compact/import/export/delete；父子 turn、stop reason、tool lifecycle、附件和 pack 选择在重启后保持一致，旧 session 有迁移路径 | 长任务和跨入口连续体验的最低要求 | 全部入口、三种交付形态 | CORE-01 | HIGH | JSONL session tree 与 legacy compatibility 已有验证基础；规模索引、并发和多用户隔离仍需 1.0 证明 |
| CORE-03 | Anthropic、OpenAI、Gemini、OpenRouter、OpenAI-compatible 和本地模型可注册、认证、选择和健康检查；tool use、vision、structured output、reasoning、streaming、context window 不支持时显式路由、降级或拒绝 | 多供应商和本地优先已批准，静默假装等价会直接破坏任务结果 | 全部入口 | CORE-01、CORE-05 | HIGH | Anthropic、OpenAI-compatible、Ollama、fake provider 有局部基础；Gemini、OpenRouter 和完整 capability negotiation 是目标能力 |
| CORE-04 | command、tool、MCP workbench、connector 通过唯一 registry 暴露 schema、版本、权限、生命周期和结构化错误；读工具可安全并发，写工具串行或隔离 | Agent 产品必须能可靠调用能力并解释失败 | 全部能力包、Headless、MCP | CORE-01、CORE-06 | HIGH | ToolRegistry、CommandRegistry、MCP lifecycle 有局部基础；新增 pack 不得建立旁路 registry |
| CORE-05 | defaults、user、project、local、env、CLI、managed policy 有确定性优先级；API key、OAuth、Cookie、SSH key 和 license key 只进入 Keychain / vault，状态接口只显示脱敏元数据 | 配置可解释、凭据不泄露是公开发行与企业交付基本要求 | Local、Cloud、Enterprise、设置页 | CORE-01 | HIGH | resolved config、managed overlays、redacted status 有局部基础；真实 OAuth、Keychain/vault 和轮换需完整实现 |
| CORE-06 | 首次启动选择安全 / 平衡 / 自治档位，可按项目覆盖；ProjectTrust、路径范围、sandbox、network policy、exec policy 和外部副作用审批统一产出 PolicyDecision；硬策略不可被任何档位绕过 | 用户必须能控制 Agent 能做什么，企业必须能证明策略未被绕过 | 全部入口、全部 pack | CORE-01、CORE-04、CORE-05 | HIGH | fail-closed trust root、permission profiles、部分 exec/network policy 有局部基础；跨平台 sandbox 与真实高风险旅程仍需证明 |
| CORE-07 | Context Builder 按任务预算生成可追溯 ContextPack；repo map、symbol/path search、impact/trace、可编辑与只读文件集、artifact graph 都能显示来源、时间、hash 和截断原因 | 长上下文不能靠“全塞进去”，Coding/Research 都需要可解释检索 | 全部 pack | CORE-02、CORE-04、CORE-06 | HIGH | deterministic repo map、index、artifact manifest、lexical/hash-vector search 有局部基础；生产 embedding、图一致性和跨源权限仍未完成 |
| CORE-08 | memory 按 Observation / Reasoning / Decision / Git / Research evidence 等类型持久化，带 source、confidence、scope、retention 和 stale 标记；用户可搜索、编辑、导出、删除，live evidence 始终优先 | 跨会话有用但不可让旧记忆覆盖当前事实 | 全部 pack、Desktop、Web | CORE-02、CORE-05、CORE-07 | HIGH | append-only local memory、redaction 和 search 有局部基础；自动提议、语义检索、失效和多用户 ACL 是目标能力 |
| CORE-09 | Intent Router 对每个请求输出 pack、运行档位、风险、权限、副作用和验收理由；简单请求可直接执行，跨步/跨会话/多 Agent/外部副作用任务创建持久 WorkflowRun DAG | 用户不应手动决定每次该用哪个内部子系统 | 全部入口、全部 pack | CORE-01、CORE-06、CORE-07 | HIGH | WorkflowRun / board 基础存在；跨三 pack router 与真实用户覆盖是目标能力 |
| CORE-10 | 任务启动时定义可观察 acceptance；Evidence Ledger 记录 diff、命令、测试、引用、实验、审批、外部回执和产物；Verifier 只能输出 complete / rework / blocked / awaiting_approval / result_unknown | 自然语言“做完了”不是完成证明 | 全部 pack、发布系统 | CORE-01、CORE-04、CORE-09 | HIGH | workflow evidence/integrity/verifier 有已验证基础；Research、Daily、Cloud 的领域 evidence 仍是目标 |
| CORE-11 | crash、interrupt、超时、provider 断流、worker 失败和外部依赖失败均可恢复；EventLog 是事实源，state 可重建；内部操作有 idempotency key，未知外部结果先查询或人工核对而非盲目重放 | 可靠长任务的必要条件，也是 Kiana 核心价值 | 全部入口、Local/Cloud/Enterprise | CORE-02、CORE-09、CORE-10 | HIGH | prefix-proof recovery 与 bounded swarm journal 有局部/验证基础；真实平台故障注入和外部 connector 补偿仍未完成 |
| CORE-12 | 多 Agent 仅接收 typed WorkPacket，包括目标、输入、allowed/forbidden paths、tools、预算、依赖、验收和返回 schema；路径锁或 worktree 隔离；集成后重新跑整体 gate | 并行不能以冲突、越界和不可审计为代价 | 全部 pack、Local/Remote worker | CORE-06、CORE-09、CORE-10、CORE-11 | HIGH | bounded swarm、path lock、worker identity 和 deterministic team smoke 有局部基础；跨平台身份、远程池和领域团队旅程仍需实现 |
| CORE-13 | skills、plugins、hooks、MCP、connectors 和 domain packs 有来源、许可证、版本、完整性、权限 manifest、安装 receipt、enable/disable/update/rollback、冲突诊断和可见性；未知项目资源默认不加载 | 可扩展性是桌面连接器和三个 pack 的共同基础，也是主要供应链风险 | CLI/TUI、Desktop、IDE、Web、Enterprise policy | CORE-04、CORE-05、CORE-06 | HIGH | skill/plugin/hook/MCP 多个本地切片已存在；真实签名信任链、connector OAuth 和稳定第三方 pack contract 未完成 |
| CORE-14 | usage、token、估算成本、延迟、retry、worker budget、tool/connector 健康、policy denial 和 audit event 可按 session/workflow/provider/tenant 查询；本地遥测默认关闭或仅本地 | 长任务、云计费和企业运维都需要可解释资源与行为记录 | 全部入口、Cloud、Enterprise | CORE-01、CORE-05、CORE-09 | MEDIUM | runtime usage 与若干 status/report 有基础；统一 usage ledger、租户计费对账和隐私控制是目标 |
| CORE-15 | Local Personal 无需账户即可运行三个 pack；代码、session、memory、workflow、evidence 默认留在设备；用户可备份、迁移、导出和彻底删除；sync/telemetry 明示 opt-in | Local-first 是产品不可退让边界 | Local、Desktop、CLI、IDE | CORE-02、CORE-05、CORE-08 | HIGH | 文件型本地持久化已有基础；完整数据目录、迁移、备份/恢复和删除证明仍需统一 |
| CORE-16 | Linux/macOS 原生、Windows/WSL 安装与卸载；升级/回滚/数据迁移；签名、checksum、SBOM、许可证/依赖扫描、release notes、doctor 和支持包；所有声明绑定 release proof | 功能存在但无法安全分发仍不是 1.0 | Local、Cloud worker、Enterprise bundle | 以上 Core 契约 | HIGH | release/package/schema smoke 与 proof contracts 有基础；真实渠道、签名、macOS/Windows runner 和客户验收仍属外部缺口 |

### Coding Pack

Coding 以 1.0 冻结日期的 Claude Code 公开功能覆盖为最低基线，但不要求命令名、配置格式、品牌或像素级 UI 兼容。冻结清单必须逐项记录 public behavior、Kiana journey、支持入口、自动化测试、人工验收和 evidence；动态上游差异进入后续版本，不允许让“对齐”成为永不关闭的描述。

| ID | 可观察的 1.0 功能 | 为什么是基本盘 | 目标入口 | 依赖 | 复杂度 | 当前证据边界 |
| --- | --- | --- | --- | --- | --- | --- |
| COD-01 | 发布一份带冻结日期的 Claude Code public-parity ledger，覆盖安装/登录、CLI flags、交互命令、IDE、Desktop/Web、models、memory、MCP、subagent/team、plugins/skills/hooks、checkpoint、Chrome、Git/CI、SDK、voice 等公开旅程；每行必须有结果证据或明确差异决策 | “完整覆盖”必须可审计、可冻结、可测试 | 全部 Coding 入口 | CORE-16 | HIGH | 当前 feature matrix 只覆盖若干能力域，不等于完整 public parity ledger |
| COD-02 | init 一个仓库后可生成/发现项目说明、agents、rules、prompts、checks 和 memory；展示作用域与优先级；未信任项目不加载自动化资源 | 用户期望 Agent 理解项目约定而不是每次重新解释 | CLI/TUI、IDE、Desktop | CORE-06、CORE-08、CORE-13 | MEDIUM | project resources 与 trust 有局部基础；统一项目初始化旅程需补齐 |
| COD-03 | 浏览目录、Read/Grep/Glob、symbol/reference、repo map、dependency/impact/trace 和图片/截图输入可组合使用；结果显示遗漏、预算和来源 | 探索代码库是 coding agent 的第一日常工作流 | 全部 Coding 入口 | CORE-03、CORE-04、CORE-07 | HIGH | repo map/search 和 vision capability contract 部分存在；跨语言 impact/trace 与真实多模态验收未完成 |
| COD-04 | 用户可固定 editable/read-only 文件、引用文件/目录/URL/图片、查看 context 使用量、compact、恢复 compact 后语义，并知道哪些内容被省略 | 可控上下文决定代码质量、成本和安全 | CLI/TUI、IDE、Desktop、Headless | CORE-02、CORE-07 | HIGH | file-set 与 context pack 有局部基础；完整上下文 UX 和质量回归需补齐 |
| COD-05 | Write/Edit/Delete/NotebookEdit 有精确 patch、AI diff 预览、changed-files 列表、read-before-write/mtime 冲突保护；每个 assistant turn 可 checkpoint、diff、undo，不能覆盖用户后续修改 | 安全可逆编辑是任何成熟 coding agent 的基本盘 | CLI/TUI、IDE、Desktop | CORE-04、CORE-06、CORE-10、CORE-11 | HIGH | 本地 coding workflow 被 matrix 标为 locally ready；仍需跨入口和平台真实旅程 |
| COD-06 | shell/PowerShell 任务支持 cwd/env、streaming、timeout、cancel、background/daemon、output truncation、sandbox/network 状态和结构化退出；危险命令在执行前被策略拒绝或审批 | 构建、测试和调试离不开终端，且终端是最高风险面之一 | CLI/TUI、IDE、Desktop、Headless | CORE-04、CORE-06、CORE-11 | HIGH | exec policy 和 Linux sandbox 有局部基础；macOS/Windows/WSL、安全边缘和后台 UX 未完成 |
| COD-07 | Git status/diff/log/branch/commit 辅助、worktree 隔离、冲突检测、PR 创建/检查/review、GitHub Actions/CI 触发均保留预览、审批和远端回执；不自动处理用户未授权改动 | 真实交付链不止修改文件 | CLI/TUI、IDE、Desktop、Web/Cloud | CORE-06、CORE-10、CORE-11、CORE-12 | HIGH | checkpoint/worktree/source-control proof 基础存在；完整远端 Git/PR/CI 旅程仍需真实服务验收 |
| COD-08 | 自动发现并运行 format/lint/typecheck/build/test；失败形成诊断和受限 repair loop；debug 支持日志、LSP diagnostics、复现步骤和最大重试；通过后再 review | 用户期望 Agent 交付可运行改动而非只生成 patch | 全部 Coding 入口 | CORE-04、CORE-09、CORE-10、COD-05、COD-06 | HIGH | isolated checks/review 与 repair loop 有局部基础；多语言黄金任务和真实 debugger/LSP 仍需扩展 |
| COD-09 | Ask、Plan、Edit/Execute 等模式及 slash command、快捷键、output style、status line、interactive question、permission prompt 都有一致行为；切换模式不丢 session 或绕过 policy | 可预测交互和逐步放权是高级用户的日常需求 | CLI/TUI、IDE、Desktop | CORE-01、CORE-02、CORE-06 | MEDIUM | CLI/REPL/TUI 有大量入口基础；完整公共交互覆盖未形成冻结验收 |
| COD-10 | MCP client/server 覆盖 tools/resources/resource templates/prompts 和 stdio/HTTP/SSE/WS；skills/plugins/hooks/commands 可安装、验证、禁用和调试；项目级资源受 trust gate | Claude Code 公开生态能力，也是企业连接内部工具的标准入口 | 全部 Coding 入口 | CORE-04、CORE-06、CORE-13 | HIGH | MCP 与 extension matrix 有较多局部证据；签名渠道、OAuth connector 和端到端插件产品仍未完成 |
| COD-11 | subagent、agent teams 和并行任务可由用户显式启动或由 router 在授权后启动；每个 worker 有隔离 scope、消息、预算、状态、取消、handoff 和集成 gate | 多 Agent 已是公开基线，同时是 Kiana 差异化支柱 | CLI/TUI、IDE、Desktop、Web/Remote | CORE-09、CORE-10、CORE-11、CORE-12 | HIGH | deterministic team/swarm 基础存在；完整人机 UX、远程池和多入口状态未完成 |
| COD-12 | Browser/Chrome、computer-use、screen capture、clipboard、URL handler、Notebook、LSP 等 workbench 可按平台探测、权限启用、暂停和审计；不可用时明确降级 | 公开 coding workflows 已超出纯终端 | Desktop、IDE、CLI/TUI | CORE-03、CORE-04、CORE-06、CORE-13 | HIGH | native integration crates 与 notebook smoke 有基础；真实平台 sandbox、浏览器扩展和可视化审批仍需证明 |
| COD-13 | 非交互 print/exec、stream-json、SDK/RPC/MCP 可在 CI 和第三方应用中创建/恢复 session、订阅 events、响应 approval、取消任务并取得 typed result；GitHub Actions 有受限凭据和 PR 回执 | 自动化和嵌入是完整 Coding 产品的公开基线 | Headless、CI、Cloud | CORE-01、CORE-02、CORE-06、CORE-10 | HIGH | SDK/MCP/stream contracts 有基础；稳定 SDK 版本、CI templates 和 live GitHub journey 未完成 |
| COD-14 | 本地任务可转为后台或 remote worker，离线后继续；用户从 CLI、IDE、Desktop、Web 查看同一进度、diff、日志和 approval，重新连接不重复副作用 | Web/Cloud/Desktop 公开旅程要求跨设备连续执行 | 全部 Coding 入口、Official Cloud | CORE-02、CORE-10、CORE-11、CLOUD-02 | HIGH | remote/bridge 和 app-server proof endpoints 有局部基础；真实 hosted worker 和跨入口 client 未验收 |
| COD-15 | voice 输入可开始、暂停、编辑确认并提交 prompt；音频权限、转写来源和保留策略可见，不可用时回退文本，不让语音绕过审批 | 本地 public-baseline 资料明确列为 Claude Code 公开功能 | CLI/TUI 或 Desktop，按平台支持 | CORE-01、CORE-05、CORE-06 | MEDIUM | 当前设计未提供实现证据，属于 public parity 目标能力 |
| COD-16 | code review、dependency/security review、test coverage 和 release review 产出带文件/行/严重度/置信度/复现与验证证据的 findings；未验证安全问题不得包装成事实 | 开发者需要可行动的 review，而不是泛化评论 | CLI/TUI、IDE、Desktop、CI | CORE-07、CORE-09、CORE-10、COD-08 | HIGH | review/audit/eval 基础存在；真实代码库黄金集、误报和安全边界需 1.0 评测 |

### Academic Research Pack

Research Pack 的最低标准不是“能总结 PDF”，而是从问题、文献、数据、实验到论文和复现包的完整 provenance 链。本地 reference 对通用 memory、artifact graph、workflow、notebook、multi-agent 和 evidence 有较强覆盖，但对学术检索、统计正确性和投稿流程的直接产品证据较弱，因此下列大多是已批准目标能力，不能从通用模块名推断为已实现。

| ID | 可观察的 1.0 功能 | 为什么是基本盘 | 目标入口 | 依赖 | 复杂度 | 当前证据边界 |
| --- | --- | --- | --- | --- | --- | --- |
| RES-01 | 研究者可定义 research question、scope、inclusion/exclusion criteria、假设、变量、伦理/数据限制、里程碑和 acceptance；变更形成 decision history | 没有明确问题与边界，后续检索和实验不可验证 | Desktop、Web、CLI/TUI | CORE-09、CORE-10 | MEDIUM | Project OS 对象可复用；学术 schema 与黄金旅程是目标 |
| RES-02 | 通过可配置 search/connectors 检索文献、数据集和代码；显示 query、来源、抓取时间、许可/访问状态；合法下载与仅记录 DOI/arXiv/URL 明确区分 | 文献发现与合法获取是研究工作入口 | Desktop、Web、Headless | CORE-05、CORE-06、CORE-13 | HIGH | Web tools/context ingest 有局部基础；学术数据库 connector、授权和合法下载尚无完整证据 |
| RES-03 | PDF、网页、supplement、表格和扫描件可解析、OCR、去重、版本关联；每个 excerpt 保留页码/section/坐标、content hash 和 parser warning | 研究结论必须能回到原文位置 | Desktop、Web、CLI | CORE-07、CORE-13 | HIGH | 通用 artifact ingest 有基础；PDF/OCR/表格质量与错误报告是目标 |
| RES-04 | 文献库支持 DOI/arXiv/ISBN/URL 元数据校验、BibTeX/RIS/CSL import/export、引用样式、重复/撤稿/版本提示；正文引用可跳回 source record | 引用管理是可发表研究的基本盘 | Desktop、Web、IDE/editor bridge | RES-02、RES-03、CORE-10 | HIGH | 当前材料没有完整 citation manager 证据 |
| RES-05 | notes、claims、counter-evidence、methods、datasets、experiments 和 artifacts 形成 evidence graph；边有 extracted/inferred/ambiguous 和 confidence；孤立 claim 被标记 | 总结文本不能替代 claim-support 关系 | Desktop、Web、CLI | CORE-07、CORE-08、CORE-10、RES-03 | HIGH | artifact graph/memory 提供底层基础；研究语义和 UI 是目标 |
| RES-06 | 系统可生成可编辑 literature review matrix、研究计划、related-work taxonomy 和 progress board；每个 synthesized statement 引用具体来源，冲突证据并列显示 | 研究者需要可审查综合，而非单一模型答案 | Desktop、Web | RES-01、RES-04、RES-05、CORE-09 | HIGH | workflow/report 可复用；真实文献集评测缺失 |
| RES-07 | 数据集和代码仓库 intake 记录版本、license、checksum、schema、split、预处理、环境和访问限制；敏感数据有本地/组织 policy | 可复现和合规从数据、代码进入工作区时开始 | Desktop、CLI、Headless、Enterprise | CORE-05、CORE-06、CORE-07、CORE-13 | HIGH | generic artifact/provenance 有基础；dataset registry 和敏感数据策略是目标 |
| RES-08 | 实验定义参数、seed、环境、输入 hash、代码 revision、资源预算和预期指标；local/remote execution 可暂停恢复；每次 run 产出 immutable result artifact | 只有可重跑实验才能支持论文结论 | CLI/TUI、Desktop、Web/Cloud | CORE-09、CORE-10、CORE-11、RES-07 | HIGH | workflow/notebook/eval 有局部基础；研究 experiment runner 和 remote resource orchestration 未完成 |
| RES-09 | notebook/data interpreter 在隔离环境运行 Python 等受支持 kernel，支持表格检查、统计检验、effect size、confidence interval、多重比较提示和图表 provenance；不会把失败 cell 当结果 | 统计与图表是实验闭环基本盘，也是高风险幻觉源 | Desktop、CLI、Web | CORE-04、CORE-06、CORE-10、RES-08 | HIGH | NotebookExecute isolation smoke 仅是基础；kernel 管理、统计保障和图表 lineage 是目标 |
| RES-10 | 支持 benchmark、baseline、ablation、error analysis、robustness 和 qualitative case workflow；metric definition、selection decision 和 negative result 均进入 ledger | 只跑最好结果无法形成可信实验 | Desktop、Web、Headless | RES-08、RES-09、CORE-10 | HIGH | offline RuntimeEvent eval 不是学术实验评测；本项是目标能力 |
| RES-11 | 论文 workspace 支持大纲、章节、LaTeX/Markdown/Word、figure/table、citation、claim-support 检查、术语一致性和 reviewer-facing diff；任何无来源数字/引用被阻塞或标记 | 论文写作需要结构化校验，不是自由生成 | Desktop、IDE/editor bridge、Web | RES-04、RES-05、RES-09、RES-10 | HIGH | 当前 reference 只有通用 writing/PM skill 线索，无完整论文工作台证据 |
| RES-12 | 一键生成但可审查的 reproducibility package：环境 lock、code/data manifest、run commands、results、licenses、limitations、artifact checksums；另生成投稿 checklist、supplement 和 response-to-reviewers 工作区 | 复现包和投稿材料是“从研究到交付”的终点 | Desktop、CLI/Headless、Web | CORE-16、RES-07 至 RES-11 | HIGH | release artifact patterns 可借鉴；学术 bundle 与投稿验收是目标 |
| RES-13 | EDA、硬件、机器人等领域 pack 可注册领域 objects、tools、rules、eval 和 views；EDA 1.0 至少覆盖需求、结构化原理图/网表、BOM/Gerber/CPL/DFM intake、risk review 和 bring-up plan，工程签字/下单保持人工审批 | 已批准领域扩展必须用同一 research/evidence 模型落地 | CLI/TUI、Desktop、Web | CORE-13、RES-01、RES-07、RES-10 | HIGH | EDA review 仅有有限本地切片；完整 EDA 语义、供应链数据和其他领域 pack 未完成 |
| RES-14 | Research verifier 对 DOI、引用、数据、实验、统计、图表和外部投稿状态逐类验证；缺证据输出 blocked/rework/unknown，禁止模型声明使论文结论自动完成 | 研究诚信是硬约束而非附加检查 | 全部 Research 入口 | CORE-10、CORE-11、RES-04 至 RES-12 | HIGH | generic verifier 基础存在；领域 verifier 与造假/污染评测是目标 |

### Daily Work Pack

Daily Pack 的最低标准是“能安全完成真实办公动作”，不是生成建议文本。任何发送、上传、删除、发布、付款、权限变化或不可逆应用操作必须能说明目标、预览、审批策略、回执和恢复状态。

| ID | 可观察的 1.0 功能 | 为什么是基本盘 | 目标入口 | 依赖 | 复杂度 | 当前证据边界 |
| --- | --- | --- | --- | --- | --- | --- |
| DAY-01 | 本地文件、笔记、待办、提醒和个人知识库可 create/read/update/search/link/archive/export；每项有来源、时间、状态和冲突处理 | 个人工作助理首先必须管理日常信息对象 | Desktop、CLI/TUI、Web | CORE-05、CORE-08、CORE-15 | HIGH | memory/tasks/file tools 有局部基础；统一 Daily object model 和 UX 是目标 |
| DAY-02 | Calendar connector 可查询 availability、创建/改期/取消事件、处理时区/重复规则/参与者；写入前展示变化并记录服务端 event ID | 日程是个人效率基本盘且易产生外部副作用 | Desktop、Web、Mobile Web | CORE-06、CORE-11、CORE-13 | HIGH | 当前无完整 calendar connector 证据 |
| DAY-03 | Email 和消息 connector 支持检索、thread/context、草稿、附件、收件人解析、发送预览、审批、发送回执和失败/未知处理；默认不自动发送 | 沟通自动化需要强身份与副作用保障 | Desktop、Web、Headless | CORE-05、CORE-06、CORE-11、CORE-13 | HIGH | 当前无完整 email/message connector 证据 |
| DAY-04 | Word/Markdown/PDF 文档、spreadsheet 和 presentation 可读取、创建、编辑、批注、导出和 diff；公式、图表、引用和版式变化有结构化 warning；不支持元素明确降级 | 办公产物是 Daily Pack 核心，不只是聊天附件 | Desktop、Web、CLI/Headless | CORE-04、CORE-07、CORE-10、CORE-13 | HIGH | awesome-agent-skills 只证明生态需求，不证明 Kiana 实现 |
| DAY-05 | meeting workflow 支持 agenda、材料包、时间提醒、经授权的录音/转写、speaker/time provenance、notes、decision、action item 和 follow-up draft；参会者隐私策略可见 | 会议到行动项是高频端到端旅程 | Desktop、Web | DAY-02、DAY-03、CORE-08、CORE-10 | HIGH | 当前无完整 meeting product 证据 |
| DAY-06 | browser/desktop automation 可完成受限导航、表单、下载/上传、clipboard 和 app control；每一步有可视状态、目标校验、timeout/cancel、checkpoint/compensation 和 receipt | 真实 Daily 工作跨应用，且 RPA 最易误操作 | Desktop、CLI/TUI、Web observer | CORE-04、CORE-06、CORE-11、CORE-13 | HIGH | browser/computer-use 基础 crate 存在；可恢复 RPA 与真实 app journeys 未验收 |
| DAY-07 | 项目/流程模板可把目标拆成 DAG、board、task、owner、deadline、approval 和 report；支持 Coding/Research/Daily 混合 task 和 bounded multi-agent | 个人与团队工作不是孤立单轮动作 | Desktop、Web、CLI/TUI | CORE-09、CORE-10、CORE-12 | HIGH | Project OS/swarm 基础可复用；Daily workflows 与真实 connector 闭环是目标 |
| DAY-08 | workspace search 跨本地文件、notes、mail、calendar、meeting 和获批 connectors，结果按权限裁剪并显示来源/新鲜度；断开 connector 后缓存按 policy 删除或降级 | 用户需要统一检索，但不能越权拼接数据 | Desktop、Web | CORE-06、CORE-07、CORE-08、CORE-13 | HIGH | 通用 search 有基础；跨 connector ACL/retention 是目标 |
| DAY-09 | connector center 展示 discover/connect/OAuth scopes/health/last sync/data access；支持 revoke、re-auth、least privilege、per-workspace enable 和 audit；凭据不进入模型上下文 | Claude Desktop-like connector experience 的基本盘 | Desktop、Web、Enterprise admin | CORE-05、CORE-06、CORE-13 | HIGH | MCP/config 基础不等于 connector 产品；本项是目标 |
| DAY-10 | approval inbox 汇总待发送、待发布、待删除、待付款、权限变更和结果未知动作；用户可 inspect/edit/approve/deny/escalate；决定同步到原 workflow | 多工作流运行时必须有统一人机控制面 | Desktop、Web、CLI/TUI | CORE-06、CORE-09、CORE-10、CORE-11 | HIGH | permission requests 有底层基础；跨 workflow approval inbox 是目标 |
| DAY-11 | Daily verifier 对目标应用的最终状态、外部 ID、回执、附件 hash、参与者和审批进行核对；无法确认时进入 result_unknown 并禁止自动重放 | 办公自动化的“完成”只能由目标系统状态证明 | 全部 Daily 入口 | CORE-10、CORE-11、DAY-02 至 DAY-10 | HIGH | generic evidence/recovery 可复用；connector-specific verifier 未完成 |
| DAY-12 | 团队空间支持共享 project/task/artifact、comments、mentions、handoff、role visibility 和 audit；个人私有对象默认不因加入团队而共享 | 官方云和企业版必须覆盖协作基本盘 | Desktop、Web、Cloud/Enterprise | CLOUD-03、ENT-03、CORE-06 | HIGH | 当前 team runtime 不等于团队产品；这是 cloud/enterprise 目标能力 |

### 产品入口与跨入口体验

| ID | 可观察的 1.0 功能 | 为什么是基本盘 | 依赖 | 复杂度 | 当前证据边界 |
| --- | --- | --- | --- | --- | --- |
| SURF-01 | CLI/REPL/TUI 支持 prompt、commands、history/resume、diff、tool cards、approval、background/workflow monitor、settings/doctor、附件和三个 pack；文本与 JSON/stream 输出互不混淆 | 高级个人用户的首要入口，也是无 GUI 环境的恢复入口 | CORE-01 至 CORE-13 | HIGH | CLI/TUI 有大量局部基础；三个 pack 和完整 terminal UX 未完成 |
| SURF-02 | Headless SDK/RPC 有版本化 API、typed events、cancel/backpressure/reconnect、approval callback、idempotency、auth 和 language-neutral examples；breaking change 有迁移策略 | CI、第三方嵌入、cloud worker 都依赖稳定自动化面 | CORE-01、CORE-02、CORE-06、CORE-11 | HIGH | SDK/MCP contracts 有局部基础；稳定版本和外部 consumer 验收缺失 |
| SURF-03 | MCP server/client 对 tools/resources/templates/prompts、auth、capability discovery、error 和 transports 做完整互操作；policy 与本地工具路径一致 | MCP 是生态和 connector 的共同协议面 | CORE-04、CORE-06、CORE-13 | HIGH | 本地 parity fixtures 较多；第三方互操作矩阵和 OAuth 未完成 |
| SURF-04 | IDE 至少交付 VS Code 与 JetBrains 的项目上下文、chat、inline/patch diff、diagnostics、review、task/workflow state、approval 和 resume；不得有独立 session/permission model | Claude Code public baseline 明确包含两类 IDE，且 coding 需要原位 diff | CORE-01、CORE-02、CORE-04、COD-03 至 COD-16 | HIGH | 当前仓库有 app-server contracts，未证明完整 IDE clients |
| SURF-05 | Desktop 提供 Claude Desktop-like conversation/workspace、files/sources、artifacts、connector center、model/settings、computer-use、approval inbox、history/search 和三个 pack views；本地 mode 无账户可用 | 桌面基线是已批准产品定义 | CORE-01 至 CORE-15、DAY-09、DAY-10 | HIGH | native integration/app endpoints 是基础；完整 Desktop 客户端是目标 |
| SURF-06 | Web/App Server 提供 conversations/workflows/events/files/artifacts/team/admin/release operations，支持实时 reconnect、RBAC、pagination 和 bounded history；local app server 与 hosted server 使用同一 contract | 远程任务、团队和企业运维需要 Web 控制面 | CORE-01、CORE-02、CORE-06、CORE-09、CORE-14 | HIGH | direct-connect 暴露许多 read contracts；完整 Web client、write journeys 与 hosted auth 未完成 |
| SURF-07 | 用户可在 CLI 创建 workflow、IDE 查看 diff、Desktop 审批、Web 观察 remote worker；所有端看到相同 IDs、events、state 和 evidence，离线编辑冲突有明确解决 | 多入口只有共享事实才是一个产品 | SURF-01 至 SURF-06、CORE-01、CORE-02、CORE-11 | HIGH | adapters 存在，但真实跨端连续旅程仍是 1.0 关键缺口 |
| SURF-08 | 所有入口有一致的 loading/empty/error/offline/degraded/permission-denied/result-unknown 状态；长内容、键盘导航、screen reader、reduced motion 和本地化可用 | 公开发行质量包含可用性和可访问性 | 各入口实现、CORE-01 | HIGH | TUI 局部状态存在；全端 accessibility/i18n 未证明 |
| SURF-09 | Linux/macOS 原生和 Windows/WSL 的路径、terminal、sandbox、Keychain、browser/native host 差异被端到端测试；unsupported capability 明示而非隐藏 | 平台差异会直接破坏工具、安全和安装 | CORE-05、CORE-06、CORE-16 | HIGH | Linux 本地证据较强；真实 macOS/Windows runner 是明确外部验收项 |
| SURF-10 | 每个入口可查看 capability/readiness、provider/connector/plugin health、policy、data location、version/update、release blockers 和支持诊断；输出脱敏且可导出 | 用户和支持团队需要知道为什么某功能不可用 | CORE-03、CORE-05、CORE-13、CORE-14、CORE-16 | MEDIUM | doctor/settings/release proof endpoints 有局部基础；跨端呈现与支持流程未闭环 |

### Official Cloud

Official Cloud 是 Local Personal 的可选扩展，不是运行本地核心的前置条件。所有云功能必须显式启用，并继续使用同一 task、event、policy、evidence 和 recovery contract。

| ID | 可观察的 1.0 功能 | 为什么是基本盘 | 依赖 | 复杂度 | 当前证据边界 |
| --- | --- | --- | --- | --- | --- |
| CLOUD-01 | 可选账户支持注册/登录/OAuth、MFA、session/device 管理、recovery、注销和账号删除；未登录仍可使用完整 Local Personal | 云服务需要身份，但不能胁迫本地用户开户 | CORE-05、CORE-15 | HIGH | auth/license status 只提供局部 readiness；真实账户服务是目标 |
| CLOUD-02 | 用户显式选择的 session/workflow/artifact/memory 可加密同步；显示同步范围、方向、冲突、last sync、设备和恢复状态；可暂停、导出、删除云端副本 | 跨设备和 remote worker 需要同步，同时必须保持数据主权 | CLOUD-01、CORE-02、CORE-05、CORE-11、CORE-15 | HIGH | sync/remote 命令基础不等于 hosted encrypted sync；威胁模型与恢复测试未完成 |
| CLOUD-03 | remote worker 接收签名 WorkPacket，在隔离 workspace 执行，stream typed events，支持 reconnect/cancel/timeout/budget；结果经 verifier 后回传，未知副作用不重放 | 官方云的核心付费价值是可靠远程执行 | CLOUD-01、CLOUD-02、CORE-06、CORE-11、CORE-12 | HIGH | remote/bridge 与本地 worker smoke 有基础；真实 worker pool、租户隔离和 live journey 未完成 |
| CLOUD-04 | team workspace 支持成员/邀请、共享 project/workflow/artifact、comments/mentions、handoff、approval 和 activity；个人与团队数据边界显式 | 团队协作是已批准云商业能力 | CLOUD-01、CLOUD-02、CORE-06、DAY-12 | HIGH | 本地 team state 不等于 hosted collaboration；本项是目标 |
| CLOUD-05 | tenant isolation 覆盖 storage、cache、queue、logs、search index、worker、connector credentials 和 support tooling；跨租户访问有自动化负面测试 | 任何团队云服务的安全底线 | CLOUD-01 至 CLOUD-04、CORE-05、CORE-06 | HIGH | 当前无 hosted tenant isolation 证明 |
| CLOUD-06 | subscription、plan、quota、usage、invoice、payment failure、trial/cancel 和 entitlement 有可审计 ledger；计费失败不删除本地数据或锁死导出 | 商业云必须能正确收费且不制造数据勒索 | CLOUD-01、CORE-14 | HIGH | local entitlement proof contract 有基础；真实 billing/account backend 未完成 |
| CLOUD-07 | 用户可设置 retention/residency、下载 portable export、删除 workspace/account 数据并取得 deletion status；备份与恢复策略明确，支持密钥轮换 | 云数据生命周期与合规是基本盘 | CLOUD-02、CLOUD-05 | HIGH | 当前无真实 hosted 数据治理证据 |
| CLOUD-08 | 服务提供 status/health、SLO、rate limit、queue visibility、incident/audit、backup restore、abuse control 和 support diagnostics；降级时本地工作继续 | 远程长任务需要可运维性，不应拖垮本地核心 | CLOUD-03、CLOUD-05、CORE-14、CORE-16 | HIGH | release ops schemas 有基础；真实线上运行证据和 owner acceptance 未完成 |

### Enterprise Self-hosted

| ID | 可观察的 1.0 功能 | 为什么是基本盘 | 依赖 | 复杂度 | 当前证据边界 |
| --- | --- | --- | --- | --- | --- |
| ENT-01 | 提供联网和 air-gapped 安装 bundle、环境 preflight、容量规划、校验和/SBOM、离线依赖、upgrade、rollback、uninstall 和数据 migration；失败可恢复到已验证版本 | 自托管产品首先必须可重复部署和维护 | CORE-16、SURF-06 | HIGH | offline manifest/proof schema 有局部基础；真实目标环境安装验收未完成 |
| ENT-02 | 支持 OIDC/SAML SSO、MFA policy、local break-glass admin、用户停用和组织/团队映射；认证故障有安全降级 | 企业身份管理是最低采购门槛 | CORE-05、CLOUD-01 或本地 identity service | HIGH | local auth status 不等于企业 SSO；本项是目标 |
| ENT-03 | RBAC 至少区分 user、workspace admin、security/policy admin、auditor、operator；可对 provider、model、tool、connector、plugin、data、remote worker 和高风险动作设置集中 policy，deny 优先 | 企业必须集中控制 Agent 能访问和执行什么 | CORE-06、CORE-13、ENT-02 | HIGH | managed policy 有局部基础；多用户角色、管理 UI 和 enforcement matrix 未完成 |
| ENT-04 | secrets 与 customer-managed keys 接入 vault/HSM/KMS，支持 scope、rotation、revocation、audit 和 zero-secret diagnostic；worker 只获得短期最小凭据 | 企业 connector/provider 凭据不可散落在文件和日志 | CORE-05、ENT-03 | HIGH | redacted metadata 基础存在；真实 vault/HSM integrations 是目标 |
| ENT-05 | append-only audit 覆盖 login、policy、approval、tool/connector、data access、admin、export 和 release；支持检索、导出、retention、legal hold 和完整性验证 | 合规和事件响应需要可证明记录 | CORE-10、CORE-14、ENT-03 | HIGH | workflow integrity 不等于多租户 audit service；本项是目标 |
| ENT-06 | 支持 outbound allowlist/proxy/custom CA、offline model/provider、private MCP/connectors、artifact quarantine 和 egress review；受限网络下 status 能解释缺失能力 | 企业环境经常无公网或有严格网络边界 | CORE-03、CORE-06、CORE-13、ENT-03 | HIGH | network policy 有局部基础；企业网络拓扑验收未完成 |
| ENT-07 | 数据 retention/residency、workspace export/delete、backup/restore、disaster recovery、schema migration 和 integrity check 有 runbook 与演练证据；恢复保留 event/evidence 因果顺序 | 自托管客户对数据负责，Kiana 必须提供可靠工具 | CORE-02、CORE-11、CORE-15、ENT-01 | HIGH | 本地文件恢复基础不足以证明企业 DR；本项是目标 |
| ENT-08 | 提供 logs/metrics/traces、health/readiness、queue/worker/provider/connector dashboard、alert、support bundle 和脱敏 remote support 流程；可接常见 observability stack | 运维团队必须定位长任务与外部集成故障 | CORE-14、ENT-01、ENT-03 | HIGH | doctor/reports 有局部基础；生产 observability 与 support process 未完成 |
| ENT-09 | license/entitlement 支持离线签发、到期宽限、续期、席位/容量核对和可审计状态；许可证问题不能破坏客户数据访问、导出或恢复 | 企业商业交付需要可运营但不能劫持数据 | CORE-05、ENT-01 | MEDIUM | license status/proof contract 有基础；真实签发/续期 backend 未完成 |
| ENT-10 | 发布企业支持矩阵、security/privacy/licensing 文档、漏洞报告与修复 SLA、管理员/用户/API 手册、迁移与破坏性变更策略；目标客户验收有签字 evidence | 企业 1.0 包含长期支持承诺而非只有二进制 | CORE-16、ENT-01 至 ENT-09 | HIGH | 文档与 proof scaffolding 有基础；客户 acceptance 和支持运营是外部门禁 |

## Differentiators

这些能力不是“有空再做”的装饰。DIF-01 至 DIF-10 直接对应已批准的 Core Value，必须在 1.0 黄金任务中被证明；DIF-11 和 DIF-12 是商业信任放大器，也应进入 1.0 门禁。

| ID | 差异化能力 | 可观察价值 | 目标范围 | 依赖 | 复杂度 |
| --- | --- | --- | --- | --- | --- |
| DIF-01 | Evidence-first completion | 用户点击 complete 可展开 acceptance 与每条 evidence；模型自述永远不能单独完成任务 | 全部 pack、全部入口 | CORE-10 | HIGH |
| DIF-02 | Durable long-task recovery | crash/断网/切端后恢复到最后可信节点；EventLog 可重建 state；partial/unknown 不冒充 Done | 全产品 | CORE-02、CORE-09、CORE-11 | HIGH |
| DIF-03 | Result-unknown side-effect safety | 邮件、付款、发布、上传等响应丢失时先查询目标系统或请求人工核对，绝不盲目重放 | Daily、Cloud、Enterprise | CORE-06、CORE-10、CORE-11、DAY-11 | HIGH |
| DIF-04 | Bounded multi-agent with integration proof | 每个 worker 的 scope、预算、路径、工具、结果和失败可查看；冲突被阻塞；集成后整体 gate 重跑 | Coding、Research、Daily | CORE-12 | HIGH |
| DIF-05 | Local-first full product without account | 个人用户离线即可运行 Core 与三个 pack，拥有 session/memory/workflow/evidence；云仅增加同步、远程和团队规模 | Local Personal | CORE-15、各 pack | HIGH |
| DIF-06 | One state model across three packs and all surfaces | 同一 workflow 可含代码、实验和办公审批；CLI/IDE/Desktop/Web 看到同一 event/evidence，不需要导入另一套任务 | 全产品 | CORE-01、CORE-02、CORE-09、SURF-07 | HIGH |
| DIF-07 | Explainable provider capability negotiation | 每次模型选择显示 why、能力差异、fallback 和质量/成本影响；不支持的工具或 vision 不会静默失败 | 全部 pack | CORE-03、CORE-14 | HIGH |
| DIF-08 | Provenance-aware context and memory | 每段上下文、记忆、claim、graph edge 都能回到 source/hash/time/confidence；stale 或 inferred 内容明显降权 | Coding、Research、Daily | CORE-07、CORE-08、RES-05 | HIGH |
| DIF-09 | Autonomy profiles with immutable hard boundaries | 用户可在安全/平衡/自治之间切换，但删除、发布、付款、凭据、外部消息等硬策略始终执行 | 全产品 | CORE-06 | HIGH |
| DIF-10 | Verifiable Research integrity | 引用、数据、实验、统计和图表进入专用 verifier；无法验证的论文 claim 被阻塞而不是润色掩盖 | Research | RES-14 | HIGH |
| DIF-11 | Audited capability superset | 38 个 reference 和专有公开基线每项都有 Adopt/Adapt/Reject、license、安全、owner、test 与 evidence，可解释为什么有或没有 | 产品治理、release | COD-01、CORE-13、CORE-16 | HIGH |
| DIF-12 | Transparent release and enterprise readiness | 用户/管理员能看到 local vs external blockers、签名/SBOM/平台/acceptance 证据；未完成能力不会被营销措辞掩盖 | 全形态 | CORE-16、CLOUD-08、ENT-10 | MEDIUM |

## Deferred Beyond 1.0

下列项目可以推迟，因为它们不构成已批准 1.0 的缺失替代。延期理由不能被用于推迟 Coding、Research、Daily、既定入口、官方云或企业自托管。

| ID | Post-1.0 功能 | 为什么推迟 | 先决条件 |
| --- | --- | --- | --- |
| POST-01 | 原生 Windows 客户端与完整 Windows sandbox | 已批准 1.0 通过 Windows/WSL 支持；原生实现需要独立安全、terminal、Keychain、native-host 和安装矩阵 | SURF-09、CORE-16 的 WSL 旅程稳定 |
| POST-02 | 原生 iOS/Android 客户端 | 1.0 可由响应式 Web 完成观察/审批；原生移动端会引入另一套发布与安全生命周期 | SURF-06、SURF-07、Cloud 稳定 |
| POST-03 | 多区域 active-active、跨云调度和极端规模 worker fleet | 1.0 需要可靠 official cloud，不需要先解决全球超大规模 | CLOUD-03、CLOUD-05、CLOUD-08 有真实负载数据 |
| POST-04 | P2P / local-only 多设备同步 | 有价值但冲突、密钥发现和网络穿透复杂；不应阻塞 1.0 的显式云同步 | CLOUD-02 同步语义稳定 |
| POST-05 | Marketplace 付费结算、分成和公开评级体系 | 1.0 需要可信扩展生命周期，不需要先商业化第三方生态 | CORE-13 签名、review、rollback、abuse 流程成熟 |
| POST-06 | 用户自训练/fine-tune 模型与训练集管理 | Kiana 1.0 是 Agent 平台而非 foundation-model 训练平台；先把 provider 和 eval 做正确 | CORE-03、CORE-14、pack eval 稳定 |
| POST-07 | 实时双向语音/视频会议 Agent 与 ambient always-on assistant | 1.0 的 voice prompt 和经授权 meeting capture 足够；常驻监听有额外隐私和资源风险 | COD-15、DAY-05、隐私评测成熟 |
| POST-08 | 新增法律、医疗、财务等高风险一方领域包 | 1.0 已承诺 Research 与 EDA/硬件/机器人扩展；新高风险领域需要单独责任与监管设计 | domain-pack contract、领域 verifier、责任边界成熟 |
| POST-09 | 专业 CAD/EDA 自动布局布线、SI/PI/热仿真和自动生产输出 | 应优先连接专业工具并保留工程师 gate；自研专业求解器会偏离核心产品 | RES-13 connector 与 evidence 工作流稳定 |
| POST-10 | 超大规模开放式 agent society / 自由群聊 | 对用户价值和可靠性未证明，且与 bounded execution 原则冲突；仅在受限研究模式评估 | CORE-12 真实数据证明需要 |
| POST-11 | 沉浸式 3D/AR 工作区和装饰性知识宫殿 UI | 不改善当前核心任务完成率，且可能掩盖 provenance 与密集工作流 | Desktop/Web 基本工作台成熟 |
| POST-12 | 完全自治的 release/采购/付款/组织权限变更 | 即使未来也只能在组织明确双人审批和可逆边界内增强；“无人批准”本身不是目标 | 不能取消 AF-08 的硬边界 |

## Anti-Features

| ID | 明确不做的能力 | 为什么看似有吸引力 | 实际问题 | Kiana 的替代方案 |
| --- | --- | --- | --- | --- |
| AF-01 | 复制 Claude Code、Claude Desktop 或其他专有产品的源码、品牌、素材或隐藏协议 | 最快获得表面 parity | 侵权、不可维护、无法独立演进 | 仅冻结公开可观察行为，clean-room 独立实现并保留差异证据 |
| AF-02 | 机械合并全部 reference 代码 | 看起来能快速形成“超集” | 许可证冲突、重复 runtime、不同安全模型、stubs 和依赖爆炸 | 每项 Adopt/Adapt/Reject；代码复用先过 license、架构、安全和测试 gate |
| AF-03 | 用命令、页面、类型、模块、测试桩或 mock response 数量证明完成 | 容易量化进度 | 产生 completion theater，真实旅程仍不可用 | requirement → journey → test → evidence → release proof 的闭环 |
| AF-04 | 为 Desktop、Web、IDE、Cloud 或每个 pack 复制模型 loop、session、permission、workflow 或 event schema | 局部团队开发更快 | 状态分叉、策略绕过、跨端无法恢复、长期重复维护 | 所有入口/pack 只扩展 shared Core contract |
| AF-05 | 强制账户、云同步或订阅才能使用本地 Core/pack | 便于增长和收费 | 违背 Local-first，制造数据与供应商锁定 | 本地完整；只对同步、remote、team scale 和 enterprise governance 收费 |
| AF-06 | 把 API key、OAuth token、Cookie、SSH key、license key 写入项目、普通日志、session 或模型上下文 | 调试和自动配置方便 | 凭据泄露与供应链攻击 | Keychain/vault、短期 token、脱敏 metadata、zero-secret diagnostics |
| AF-07 | 自治模式、hook、plugin、MCP、managed override 或 remote worker 绕过 ProjectTrust、sandbox、network/exec policy 或硬审批 | 追求“完全自主” | 任意代码执行、数据外泄和不可逆损失 | autonomy 只能在 hard policy 内增加预算与自动化程度 |
| AF-08 | 自动执行或重放删除、发送、发布、付款、采购、权限变化、push/merge/deploy 等高风险动作而无明确策略/审批/回执 | 演示效果强、节省点击 | 目标错误、重复副作用、法律和财务风险 | preview + scoped approval + idempotency + receipt + result_unknown query/人工核对 |
| AF-09 | 生成不存在的 DOI/引用/数据/实验/显著性/图表/审稿状态或任务完成状态 | 结果更“完整” | 学术不端和错误决策 | provenance verifier；无证据时 rework/blocked/unknown |
| AF-10 | 无界 agent loop、无限 worker、自由 speaker 群聊或无预算 retry | 看起来更智能、更并行 | 成本失控、冲突、重复失败和无法归责 | bounded WorkPacket、max workers、budget、termination、path lock、integration gate |
| AF-11 | 让 memory、embedding 或 knowledge graph 覆盖 live file、git、test、外部状态或原始文献 | 检索更快、答案更流畅 | stale/inferred 数据被当事实 | source priority、freshness、confidence、live refresh 和可点击原始证据 |
| AF-12 | 未签名、无 provenance/permission manifest 的 marketplace 自动安装或自动更新 | 降低扩展安装摩擦 | 供应链和权限升级攻击 | 来源/版本/hash/signature/receipt/policy/review/rollback |
| AF-13 | 隐藏 telemetry、默认上传代码/session/memory，或用模糊文案捆绑 sync | 便于产品分析与训练 | 隐私、信任和企业合规失败 | 明示 opt-in、数据预览、按类开关、retention/export/delete |
| AF-14 | 把不同 provider 的 tool、vision、context、reasoning 或 structured output 当作完全等价 | UI 简单、路由方便 | 任务在弱能力模型上静默降质或失败 | capability negotiation、routing、visible fallback、明确拒绝 |
| AF-15 | 默认启动攻击性 pentest、漏洞利用、凭据测试或外部扫描 | “安全 Agent”演示醒目 | 越权、法律和生产风险 | 默认 defensive review；攻击性操作需明确授权、scope、隔离和证据留存 |
| AF-16 | 自动 commit 用户全部 dirty state、reset/checkout、清理 untracked 或覆盖 agent 结束后的用户修改 | Git 自动化看起来顺滑 | 数据丢失和归属不明 | touch-set、checkpoint、isolated worktree、late-edit conflict、明确批准 |
| AF-17 | 让 Research/EDA Agent 替代专业工程师签字、自动下单或承诺器件供应/安全 | 提供“一键交付”错觉 | 工程责任、实时供应链和人身安全风险 | 结构化 review、风险提示、专业工具 connector、人工 approval/sign-off |
| AF-18 | 为追求像素级/命令级兼容而冻结 Kiana 自己的 UX 和配置模型 | 迁移看似零成本 | 绑定专有产品演进并妨碍统一三 pack 架构 | 能力与真实 journey 对齐，提供必要 migration/import 而非永久克隆 |

## Feature Dependencies

~~~text
S0 共享契约冻结
    CORE-01 typed events
      -> CORE-02 session tree
      -> CORE-03 provider capability model
      -> CORE-04 command/tool/workbench registry

S1 数据与安全边界
    CORE-05 config/secrets
      -> CORE-06 trust/policy/autonomy/sandbox
      -> CORE-15 local data lifecycle

S2 可靠执行闭环
    CORE-07 context/provenance + CORE-08 memory
      -> CORE-09 router/workflow
      -> CORE-10 evidence/verifier
      -> CORE-11 recovery/idempotency/result_unknown
      -> CORE-12 bounded multi-agent
      -> CORE-13 extension/connector/domain-pack lifecycle

S3 能力包并行垂直切片
    Coding public-baseline journeys (最高投入)
    Research source -> claim -> experiment -> paper/repro journeys
    Daily connector -> preview -> side effect -> receipt journeys
    三者共享 S0-S2，不创建第二运行时

S4 产品入口
    CLI/TUI + Headless 可较早验证契约
      -> IDE
      -> Desktop
      -> Web/App Server
      -> SURF-07 cross-surface continuity

S5 商业形态
    Official Cloud identity/sync -> remote worker/team/billing/ops
    Enterprise bundle/identity -> RBAC/policy/vault/audit/DR/ops

S6 收敛与 1.0
    38-reference ledger + public-parity ledger
      -> golden journeys + failure injection
      -> platform/install/upgrade/rollback/security/supply-chain proof
      -> target-user, cloud-owner, enterprise-owner acceptance
~~~

### 关键依赖说明

| 上游能力 | 下游能力 | 为什么必须先完成 |
| --- | --- | --- |
| CORE-01 + CORE-02 | SURF-01 至 SURF-07 | 入口只有先共享 event/session 身份，才能实现真实 resume、replay 和跨端连续性 |
| CORE-03 | 三个 pack、Cloud remote worker | pack 需要按 tool/vision/context/structured-output 能力选择模型，remote 不能把 provider 差异留给客户端猜测 |
| CORE-05 + CORE-06 | CORE-13、DAY connectors、Cloud/Enterprise | project resources、OAuth connectors 和 remote execution 在 trust/secrets/policy 前启用会形成直接安全漏洞 |
| CORE-07 + CORE-08 | Coding repo workflow、RES-05、DAY-08 | 搜索、memory 和 graph 必须先有 provenance/freshness，才能安全进入代码修改、研究 claim 和跨 connector 检索 |
| CORE-09 + CORE-10 | CORE-12、COD-11、RES-08、DAY-07 | 多 Agent 与长任务需要明确 DAG、验收和 evidence，否则并行只是不可审计的聊天 |
| CORE-10 + CORE-11 | DAY-02/03/06/11、CLOUD-03 | 外部副作用必须先有 receipt、idempotency 和 result_unknown 语义，才能安全自动化或远程重试 |
| RES-02 至 RES-05 | RES-06、RES-11、RES-14 | 只有先建立可定位 source/citation/claim graph，综合写作和 research verifier 才有事实基础 |
| RES-07 至 RES-10 | RES-11 + RES-12 | 论文结果、图表和复现包依赖版本化数据、代码、环境和实验 artifacts |
| DAY-09 | DAY-02/03/05/08 | Calendar、mail、meeting 和跨源搜索都依赖 connector auth/scope/health/revoke 的统一产品契约 |
| SURF-02 + SURF-06 | CLOUD-03、ENT-08 | remote worker、Web console 和企业运维需要稳定 server/events API，而不能 shell out 或读取内部文件 |
| CLOUD-01 + CLOUD-02 | CLOUD-03 + CLOUD-04 | remote worker 和 team space 需要身份、设备、同步范围和冲突语义先稳定 |
| ENT-01 至 ENT-03 | ENT-04 至 ENT-10 | vault、audit、DR、ops 和 support 必须绑定可部署实例、身份与角色，否则无法定义租户/管理员边界 |
| CORE-16 | 任何“1.0 complete”声明 | 安装、迁移、签名、平台和支持 evidence 是功能的一部分，不是代码完成后的可选包装 |

### 并行开发约束

- Coding、Research、Daily 可以在 S0-S2 契约稳定后并行；Coding 获得最高投入，但三个 pack 共同进入 1.0。
- Surface 团队可以尽早做 thin client 和 contract fixture，但如果需要复制 provider loop、permission、session 或 workflow，必须停止并回到 Core。
- Cloud 与 Enterprise 可以共用 task/event/policy/evidence contract 和部署组件，但 tenant/RBAC/retention/licensing 语义必须显式分层，不能用云端私有状态替代 Core。
- Reference adoption 可以与实现并行，但任何引入代码必须先完成 license、来源、security、owner 和 verification 条目；未审计 reference 不得进入完成统计。

## 1.0 Boundary And Roadmap Prioritization

### 1.0 范围原则

- 所有 Table Stakes（CORE、COD、RES、DAY、SURF、CLOUD、ENT）均属于正式 1.0。
- DIF-01 至 DIF-12 均进入 1.0 成功指标；其中 evidence、recovery、result_unknown、bounded multi-agent、local-first 和 research integrity 是硬门禁。
- POST-01 至 POST-12 明确在 1.0 后；它们不能替代或延迟已批准范围中的某一整条产品线。
- Alpha/Beta 可只开放部分入口或 pack，但必须标注支持矩阵、数据兼容、已知限制和升级路径；不得使用“完整”或“生产就绪”。

### 路线图优先级矩阵

| 顺序 | Feature family | 用户价值 | 实现成本 | 1.0 优先级 | 退出条件 |
| --- | --- | --- | --- | --- | --- |
| S0 | CORE-01 至 CORE-04 | HIGH | HIGH | P0 foundation | schema golden tests；CLI/SDK/MCP/remote 共享事件；provider/tool capability 可协商 |
| S1 | CORE-05、CORE-06、CORE-15 | HIGH | HIGH | P0 safety | trust/policy/secrets/data lifecycle fail closed；Linux/macOS/WSL 负面测试 |
| S2 | CORE-07 至 CORE-13 | HIGH | HIGH | P0 differentiator foundation | durable workflow 从 intent 到 verified completion；crash/unknown/multi-agent failure tests |
| S3-A | COD-01 至 COD-16 | HIGH | HIGH | P0/P1，最高投入 | 冻结 public ledger 全部 disposition；Coding golden journeys 全端通过 |
| S3-B | RES-01 至 RES-14 | HIGH | HIGH | P1，同属 1.0 | 文献到 claim、实验到论文/复现、EDA 领域 journeys 通过 integrity gate |
| S3-C | DAY-01 至 DAY-12 | HIGH | HIGH | P1，同属 1.0 | 本地办公、connector 写入、RPA unknown 和团队 handoff journeys 通过 |
| S4 | SURF-01 至 SURF-10 | HIGH | HIGH | P1，同属 1.0 | 每个入口完成核心 journey；SURF-07 跨入口连续体验通过 |
| S5-A | CLOUD-01 至 CLOUD-08 | HIGH | HIGH | P1 commercial | hosted identity/sync/remote/team/billing/ops live acceptance |
| S5-B | ENT-01 至 ENT-10 | HIGH | HIGH | P1 commercial | target self-hosted install/upgrade/SSO/RBAC/audit/backup/support acceptance |
| S6 | DIF-11、DIF-12、CORE-16 | HIGH | HIGH | P0 release gate | 38/38 reference disposition、全量 failure injection、平台/供应链/客户 evidence 完整 |
| Post | POST-01 至 POST-12 | MEDIUM | HIGH | P2+ | 仅在 1.0 基础稳定且有真实需求/风险设计后进入 |

优先级中的 P0/P1 表示依赖与投入顺序，不表示 P1 可从 1.0 删除。

### 1.0 黄金旅程与发布门禁

| Gate | 最小可观察验收 |
| --- | --- |
| Core reliability | 同一复杂 workflow 经 kill/restart、provider 断流、worker 失败和 state projection 损坏后，要么从可信 EventLog 恢复，要么 fail closed；不得重复外部副作用 |
| Coding parity | 冻结 public ledger 每一项为 verified / intentionally different / legally rejected；至少覆盖 repo explore→edit→test→review、worktree team、Git/PR/CI、browser/computer-use、SDK/headless、remote/background、IDE/Desktop/Web、voice fallback |
| Research integrity | 从系统检索并合法取得文献，构建 citation/claim graph，运行版本化实验和统计，产出论文片段与 reproduction package；注入假 DOI、错页码、篡改结果、失败 cell 时必须阻塞 |
| Daily side effects | 本地 notes/tasks/docs 正常；calendar/email/message/browser workflows 展示目标与 preview，审批后有服务端 receipt；模拟响应丢失进入 result_unknown，不能二次发送 |
| Cross-surface | CLI 创建混合 workflow，IDE 查看代码 diff，Desktop 查看 research artifact 并审批邮件，Web 观察 remote worker；四端 ID/state/event/evidence 一致，断线重连不丢状态 |
| Official Cloud | 新账户 opt-in sync，两台设备冲突可解释；remote worker 隔离执行；team member 权限生效；usage/invoice 对账；用户可 export/delete；备份恢复演练通过 |
| Enterprise | 在目标 air-gapped/受限网络安装，接 SSO，验证 RBAC/central policy/vault/audit/retention，执行升级回滚和 backup restore，生成脱敏 support bundle；客户 owner 签收 |
| Platform and supply chain | Linux/macOS 原生与 Windows/WSL 安装、升级、回滚、卸载；签名/checksum/SBOM/license scan；插件/connector provenance；无未处置 P0/P1 或已知数据损坏路径 |
| Reference governance | 38 个 immediate reference 目录和 Claude public baseline 全部有 owner、Adopt/Adapt/Reject、license、实现/测试/风险/evidence；目录存在或代码复制不算通过 |

## Reference Coverage Appendix

治理词含义：Adopt 表示可在逐文件许可证、依赖和架构复核后复用实现或直接采用机制；Adapt 表示只吸收机制并按 Kiana 契约重做；Behavior-only 表示仅观察公开/本地行为，不复用代码；Reject 表示明确拒绝某个机制而不是否定整个项目。

| # | Reference | 已检查的 live evidence | 对 Kiana 的能力贡献 | 治理决定、边界与 feature 依赖 |
| --- | --- | --- | --- | --- |
| 1 | 12-factor-agents | reference/12-factor-agents/README.md；content/factor-* | own prompt/context/control flow、统一执行状态、launch/pause/resume、small focused agents | Adapt（Apache-2.0）：用于 CORE-01、CORE-09、CORE-11、CORE-12；Reject 把原则集变成新的 runtime 依赖 |
| 2 | Archon | reference/Archon/README.md；archon-core/src；archon-*/src | 当前快照是多语言 dependency/blast-radius 工具，提供 deterministic graph、impact、diff、domain/cycle/hotspot | Adopt/Adapt（MIT）：用于 CORE-07、COD-03、COD-16；修正旧审计中将其主要归为 Project OS 的过时描述；graph 不替代 source/test evidence |
| 3 | ECC | reference/ECC/README.md；.agents/plugins；.claude；.codex-plugin | cross-harness skills/agents/hooks/MCP、operator status、安全与 policy 指南 | Adapt（MIT）：用于 CORE-06、CORE-13、SURF-10、ENT-03；只吸收 manifest/status/policy，不复制庞大 catalog 或把 catalog 当实现 |
| 4 | GitNexus | reference/GitNexus/ARCHITECTURE.md；README.md；MCP/graph source tree | repo knowledge graph、context/impact/trace/detect_changes、staleness、semantic/search pipeline | Behavior-only / clean-room（PolyForm Noncommercial-1.0）：用于 CORE-07、COD-03、COD-16；商业产品不得直接复用受限代码，graph edge 必须带来源/新鲜度 |
| 5 | MemPalace | reference/MemPalace/README.md；src/core；src/storage；src/mcp | local memory layering、init/mine/search/status、semantic retrieval | Adapt（ISC）：用于 CORE-08、DIF-08；Reject 不可审计的空间隐喻成为核心 schema，必须保留 source/confidence/stale |
| 6 | MetaGPT | reference/MetaGPT/README.md；metagpt/actions；metagpt/roles | role/action/team、PRD/design/code/test artifacts、research/data-interpreter 线索 | Adapt（MIT）：用于 CORE-12、RES-01、RES-06 至 RES-10；Reject 角色名堆叠、无 scope 协作和重型 Python runtime 侵入 Core |
| 7 | OpenHands | reference/OpenHands/README.md；openhands/app_server；enterprise/LICENSE | app server、workspace/sandbox、conversation/event projection、Web control plane、human/PR boundary | Adapt；非 enterprise 部分 MIT，enterprise 另有许可证：用于 SURF-05、SURF-06、CLOUD-03、ENT-01；不得复制受限 enterprise 代码或先造第二控制面 |
| 8 | OpenSpec | reference/OpenSpec/README.md；docs；openspec/changes | proposal/design/spec/tasks/code/test/evidence artifact lifecycle 与跨 repo spec store | Adapt（MIT）：用于 CORE-09、CORE-10、RES-01、RES-12；参考中的 beta/placeholder 内容不能作为完成证据 |
| 9 | Roo-Code | reference/Roo-Code/README.md；.roo；src；package.json | IDE modes、rules、safe edit/checkpoint、diff preview、MCP 与任务 shell | Adapt（Apache-2.0，当前项目已停止维护）：用于 COD-05、COD-09、COD-10、SURF-04；不绑定 VS Code-only state，不以归档上游作为动态基线 |
| 10 | ai-coding-guide | reference/ai-coding-guide/README.md；claude-code；codex | 本地公开功能目录、中文 onboarding、权限/安全/MCP/subagent/plugins/team/checkpoint/Chrome/Git/SDK/voice/enterprise 教程线索 | Discovery/Adapt（MIT）：主要支持 COD-01、COD-15、SURF-04/05/06、文档；教程不是官方实现或完成证据，需冻结行为并独立验收 |
| 11 | aider | reference/aider/README.md；aider/repomap.py；aider/coders | repo map、editable/read-only files、Git safety、diff/undo、lint/test repair、images/URLs | Adopt/Adapt（Apache-2.0）：用于 CORE-07、COD-03 至 COD-08；不照搬 model-specific edit format，不自动 commit 用户未确认变更 |
| 12 | architect-loop | reference/architect-loop/README.md；DESIGN.md；docs/checks；skills | fresh strategist/builder context、run manifest、worktree、frozen checks、typed job evidence、watchdog | Adapt（MIT）：用于 CORE-10 至 CORE-12、COD-11、DIF-01/02/04；Reject weak sandbox、无硬 approval 和 worker 自证通过 |
| 13 | autogen | reference/autogen/README.md；python/packages；LICENSE-CODE | AgentRuntime/AgentChat、team/handoff/termination、workbench、replay/eval | Adapt；code MIT、文档 CC-BY，且当前 maintenance mode：用于 CORE-04、CORE-12、RES-08/10；Reject 自由 speaker、无界群聊和依赖已停更 Studio |
| 14 | awesome-agent-skills | reference/awesome-agent-skills/README.md | docx/pptx/xlsx/pdf、design、testing、MCP 等 skill catalog，证明扩展需求类别 | Discovery only（MIT）：用于 CORE-13、DAY-04；catalog 条目不证明质量、许可兼容或 Kiana 功能，启用前逐 skill eval |
| 15 | claude-code-main (2) | reference/claude-code-main (2)/claude-code-main/README.md；plugins；.claude-plugin；LICENSE.md | Claude Code 公开产品/插件/settings/hooks/sandbox fixture 与行为基线 | Behavior-only（Anthropic 专有条款）：用于 COD-01、COD-02、COD-09/10；Reject 源码、品牌、素材复制，只做公开行为 clean-room 实现 |
| 16 | claude-code-rev-main | reference/claude-code-rev-main/README.md；src/assistant；src/commands；src/context；src/tools | restored query/tool loop、tool_use/result pairing、structured IO、surface organization | Behavior-only（根目录无明确许可证，且 restored source）：用于 CORE-01/02/04、COD-01；不复用代码，不信任隐藏 stub/server，避免巨型 runner |
| 17 | claude-code-rust | reference/claude-code-rust/README.md；src；Cargo.toml；DEPLOYMENT_COMPLETION.md | Rust portability、single-binary/i18n/WASM/product-shell 设想 | Audit then Adapt（MIT）：用于 CORE-16、SURF-08/09；性能和“完整”声明必须用 live code/test 复核，stub/mock 和重复 vendor tree 不进入完成统计 |
| 18 | claude-mem-candidate | reference/claude-mem-candidate/README.md；src/claude_memory；tests | session memory candidate extraction、FTS/semantic/hybrid search、consolidation | Adapt（MIT）：用于 CORE-08；候选 memory 只能成为带 provenance 的 proposal，不能自动进入高置信上下文 |
| 19 | claude-memory | reference/claude-memory/README.md；src/claude_memory；tests | cross-session extraction、FTS5、semantic search、knowledge graph、replay、MCP/dashboard | Adapt（MIT）：用于 CORE-08、SURF-05/06；必须增加 secret/PII、stale、scope 和多用户 ACL，不直接生成权威项目事实 |
| 20 | cline | reference/cline/README.md；apps/cli；apps/vscode；apps/examples/multi-agent | CLI/VS Code/SDK、ToolExecutor、patch/checkpoint、Kanban/worktree、多 Agent 示例 | Adapt（Apache-2.0）：用于 CORE-04、COD-05/11/13、SURF-02/04；不绑定 IDE-only runtime 或复制旧/新架构并存的重复状态 |
| 21 | codex | reference/codex/README.md；codex-rs；docs；.codex/skills | Rust agent architecture、protocol/app-server、thread/turn/item、exec policy、sandbox/trust、MCP、TUI、release hardening | Adopt/Adapt（Apache-2.0）：用于 CORE-01 至 CORE-06、CORE-16、SURF-01/02/06；不耦合 OpenAI/ChatGPT 私有云语义 |
| 22 | continue | reference/continue/README.md；core/indexing；core/edit；.continue/agents/checks/rules/prompts | IDE rules/checks/prompts、indexing、diff/edit、terminal security、review automation | Adapt（Apache-2.0，当前 read-only/归档）：用于 CORE-07、COD-03/08/10、SURF-04；不依赖未完成 next-edit 或归档服务 |
| 23 | emdash | reference/emdash/README.md；PLANNING.md；agents/architecture | 多 coding agents 的 worktree/branch 隔离、diff/PR/CI、remote SSH、轻量 Desktop product shell、read-only plan mode | Adapt（Apache-2.0）：用于 CORE-12、COD-07/09/11/14、SURF-05；UI 必须消费 Core，不复制 agent/session state |
| 24 | everything-claude-code | reference/everything-claude-code/README.md；agents；commands；hooks；mcp-configs | commands/skills/agents/rules/review/test/verification 的宽能力目录 | Behavior-only / discovery（未发现根 LICENSE）：用于 COD-01/10/16、CORE-13；许可证明确前不复用代码，catalog 也不等于功能完成 |
| 25 | get-shit-done | reference/get-shit-done/README.md；agents；workflows；bin | capture/spec/plan/execute/verify/ship/review、phase/milestone、workstreams 与持久规划 | Adapt（MIT）：用于 CORE-09/10、DAY-07、DIF-01；阶段命令是 UX，不应成为独立于 WorkflowRun 的第二状态机 |
| 26 | graphify | reference/graphify/README.md；ARCHITECTURE.md；graphify；BENCHMARKS.md | deterministic local AST graph、EXTRACTED/INFERRED/AMBIGUOUS confidence、graph/report、无默认 vector store | Adopt/Adapt（MIT）：用于 CORE-07、COD-03/16、DIF-08；推断边降权，graph 不替代源码、测试或原文 |
| 27 | gsd-core | reference/gsd-core/README.md；docs/ARCHITECTURE.md；docs/COMMANDS.md；bin | capability registry、phase loop、planning/research/execute/verify/ship、跨 runtime adapters | Adopt/Adapt（MIT）：用于 CORE-09/10/13、DIF-11；capability 声明、skill 文件或 phase 文档本身不算运行功能 |
| 28 | gstack | reference/gstack/README.md；ARCHITECTURE.md；SKILL.md；browse；agents | plan/eng/design review、QA/browser、ship/canary/deploy/retro、frozen gate 与 release 操作 | Adapt（MIT）：用于 COD-16、CORE-10/16、DIF-01/12；小任务不强制重流程，高风险 ship/deploy 仍需 Kiana approval |
| 29 | herdr | reference/herdr/README.md；SKILL.md；src；LICENSE | durable terminal panes、agent status、detach/reattach、socket API、remote SSH 和 plugin UX | Behavior-only（AGPL-3.0-or-later / commercial dual license）：用于 COD-06/11/14、SURF-01/05；商业代码复用需明确 AGPL 策略或商业许可 |
| 30 | langchain | reference/langchain/README.md；libs/core；libs/model-profiles；libs/standard-tests | provider/tool/retriever/vector integration 抽象、model profiles 和 contract tests | Adapt（MIT）：用于 CORE-03/04/07/13；只做 adapter/contract 参考，Reject 用框架黑盒接管 Kiana control flow |
| 31 | memorix | reference/memorix/README.md；docs/ARCHITECTURE.md；docs/GIT_MEMORY.md | Observation/Reasoning/Git memory、source-aware retrieval、MCP/dashboard、locks、verification、multi-agent coordination | Adapt（Apache-2.0）：用于 CORE-08/12、DIF-08、SURF-05/06；不自动摄入全部会话，不让 dashboard 成为事实源 |
| 32 | pi | reference/pi/README.md；packages/agent；packages/ai；packages/coding-agent | 多 provider API、agent core/session、interactive TUI、RPC/extension、sandbox choices、supply-chain/release hardening | Adopt/Adapt（MIT）：用于 CORE-01/02/03/13/16、SURF-01/02；不采用缺省无 sandbox 的安全姿态或照搬 npm extension runtime |
| 33 | planning-with-files | reference/planning-with-files/README.md；commands；.codex/hooks | task_plan/findings/progress 持久文件、session recovery、isolated plan、autonomous/gated completion | Adapt（MIT）：用于 CORE-09/11、DIF-02；Markdown 仅做人读 artifact，EventLog/typed state 才是事实源 |
| 34 | pm-skills | reference/pm-skills/README.md；skill-manifest.json；_workflows | PRD、product discovery、roadmap、stakeholder/project management skill 模板 | Adapt after eval（Apache-2.0）：用于 RES-01/06、DAY-07、CORE-13；PM 文档不证明实现，skill 要经过版本/权限/质量评测 |
| 35 | ruflo | reference/ruflo/README.md；plugins；crates；.claude-plugin | swarm/workflow、plugins、memory、browser、security、cost/witness、federation 的大能力集合 | Selective Adapt（MIT 根许可，组件仍逐项复核）：用于 CORE-12/13/14、COD-11/12、DIF-04/11；Reject 一次性照搬、夸张数量指标和隐式 autopilot |
| 36 | skills | reference/skills/README.md；docs/engineering；docs/productivity；.claude-plugin | focused engineering/productivity workflows、TDD/debug/review/spec 和 skill packaging | Adapt after eval（MIT）：用于 CORE-13、COD-08/16；skill 指令不替代 Core enforcement、测试或领域 verifier |
| 37 | strix | reference/strix/README.md；strix/core；strix/agents；benchmarks | validated security findings、PoC/evidence、CI security gate、agent hooks/session | Adapt defensive evidence only（Apache-2.0）：用于 COD-16、DIF-01；Reject 默认攻击性自治，任何 active test 必须明确授权/scope/隔离 |
| 38 | superpowers | reference/superpowers/README.md；skills；docs/testing.md | brainstorming、plans、TDD、debugging、parallel agents、review、verification-before-completion | Adopt/Adapt（MIT）：用于 CORE-10/12、COD-08/16、DIF-01/04；按任务风险选择流程，不把所有小任务重流程化 |

**覆盖校验：** 38 / 38 个 reference immediate child directory 均有 live evidence、能力归因、治理边界和 feature 映射。

## Confidence And Evidence Gaps

| 领域 | 置信度 | 说明 |
| --- | --- | --- |
| 已批准产品范围 | HIGH | .planning/PROJECT.md 与 2026-07-14 完整产品设计对 1.0 范围、商业形态、安全和依赖顺序一致 |
| 当前 Kiana 架构与局部能力 | HIGH | codebase map、migration roadmap、feature matrix 和 live tree 交叉检查；本文仍避免把局部基础写成端到端完成 |
| 38 reference 能力归因 | MEDIUM-HIGH | 已逐目录检查 README/代表 docs/source layout/license；部分目录动态、归档、维护模式、专有或无明确 license，已降级为 behavior-only |
| Claude public baseline 枚举 | MEDIUM | 本地公开行为/教程与专有插件快照可形成冻结清单，但没有实时官方文档检索；COD-01 要求在实现阶段形成版本化 public-parity ledger |
| Research/Daily 市场 table stakes | MEDIUM | 主要来自已批准产品设计、通用 reference 机制和可验证工作流推导；本地 reference 对学术数据库、统计和办公 connector 的直接产品覆盖较弱 |
| Cloud/Enterprise 运营细节 | MEDIUM | 范围由批准设计确定；真实 tenant、billing、SSO、vault、DR 和客户 acceptance 必须在对应阶段用 live 环境提升置信度 |

## Sources

### Kiana authoritative inputs

- .planning/PROJECT.md
- docs/superpowers/specs/2026-07-14-kiana-complete-ai-agent-product-design.md
- .planning/codebase/ARCHITECTURE.md
- .planning/codebase/INTEGRATIONS.md
- .planning/codebase/CONCERNS.md
- docs/reference-migration-roadmap.md
- docs/reference-feature-matrix.md
- docs/superpowers/specs/2026-07-09-kiana-personal-project-os-design.md
- docs/reference_audit/kiana_personal_project_os_reference_audit_2026-07-09.md

### Local ecosystem evidence

- reference/ 下 38 个 immediate child directory 的 README、代表 architecture/design 文档、source layout 和 license 文件；逐项路径见 Reference Coverage Appendix。
- 无 Brave、Firecrawl、Exa 实时检索；本文未把训练记忆或未验证市场事实写成权威结论。

---
*Feature research for Kiana complete AI Agent platform, 2026-07-14.*
