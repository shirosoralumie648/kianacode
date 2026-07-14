# Requirements: Kiana

**Defined:** 2026-07-15
**Core Value:** Kiana 必须在覆盖 Claude Code 公开核心能力的基础上，更可靠地完成真实长任务，并用可验证证据和可恢复状态证明任务确实完成。
**Scope source:** `.planning/research/FEATURES.md` and the approved complete-product design

## User Stories

- 作为高级个人开发者，我可以让 Kiana 在真实代码库中理解、修改、验证和交付代码，并在失败或切换入口后可靠恢复。
- 作为研究者，我可以从问题、文献和数据推进到实验、论文与复现包，并检查每个结论的来源和证据。
- 作为知识工作者，我可以让 Kiana 安全处理文件、日历、邮件、会议、办公文档和跨应用流程，并掌握每次外部写入。
- 作为团队成员，我可以共享任务、产物、审批和 handoff，同时保留个人数据与团队数据的清晰边界。
- 作为企业管理员，我可以自托管 Kiana，并集中管理身份、权限、策略、密钥、审计、数据生命周期和运维。
- 作为本地优先用户，我可以无需账户使用完整个人产品，并自主备份、迁移、导出和删除数据。
- 作为平台与扩展开发者，我可以通过稳定的 SDK、RPC、MCP、plugin、skill、hook 和 domain-pack contracts 扩展 Kiana，而不复制核心运行时。

## v1 Requirements

以下 98 项共同构成首个正式版本。每项都必须从 source existence 逐级推进到适用的 local、target-environment 和 user-value proof；低等级证据不能替代高等级证明。

### Kiana Core

- [ ] **CORE-01**: 所有 turn、stream delta、tool call/result、permission、error、usage 和 terminal result 都输出版本化 typed RuntimeEvent；每个入口能重放同一事件序列
- [ ] **CORE-02**: session 可 create/list/resume/fork/compact/import/export/delete；父子 turn、stop reason、tool lifecycle、附件和 pack 选择在重启后保持一致，旧 session 有迁移路径
- [ ] **CORE-03**: Anthropic、OpenAI、Gemini、OpenRouter、OpenAI-compatible 和本地模型可注册、认证、选择和健康检查；tool use、vision、structured output、reasoning、streaming、context window 不支持时显式路由、降级或拒绝
- [ ] **CORE-04**: command、tool、MCP workbench、connector 通过唯一 registry 暴露 schema、版本、权限、生命周期和结构化错误；读工具可安全并发，写工具串行或隔离
- [ ] **CORE-05**: defaults、user、project、local、env、CLI、managed policy 有确定性优先级；API key、OAuth、Cookie、SSH key 和 license key 只进入 Keychain / vault，状态接口只显示脱敏元数据
- [ ] **CORE-06**: 首次启动选择安全 / 平衡 / 自治档位，可按项目覆盖；ProjectTrust、路径范围、sandbox、network policy、exec policy 和外部副作用审批统一产出 PolicyDecision；硬策略不可被任何档位绕过
- [ ] **CORE-07**: Context Builder 按任务预算生成可追溯 ContextPack；repo map、symbol/path search、impact/trace、可编辑与只读文件集、artifact graph 都能显示来源、时间、hash 和截断原因
- [ ] **CORE-08**: memory 按 Observation / Reasoning / Decision / Git / Research evidence 等类型持久化，带 source、confidence、scope、retention 和 stale 标记；用户可搜索、编辑、导出、删除，live evidence 始终优先
- [ ] **CORE-09**: Intent Router 对每个请求输出 pack、运行档位、风险、权限、副作用和验收理由；简单请求可直接执行，跨步/跨会话/多 Agent/外部副作用任务创建持久 WorkflowRun DAG
- [ ] **CORE-10**: 任务启动时定义可观察 acceptance；Evidence Ledger 记录 diff、命令、测试、引用、实验、审批、外部回执和产物；Verifier 只能输出 complete / rework / blocked / awaiting_approval / result_unknown
- [ ] **CORE-11**: crash、interrupt、超时、provider 断流、worker 失败和外部依赖失败均可恢复；EventLog 是事实源，state 可重建；内部操作有 idempotency key，未知外部结果先查询或人工核对而非盲目重放
- [ ] **CORE-12**: 多 Agent 仅接收 typed WorkPacket，包括目标、输入、allowed/forbidden paths、tools、预算、依赖、验收和返回 schema；路径锁或 worktree 隔离；集成后重新跑整体 gate
- [ ] **CORE-13**: skills、plugins、hooks、MCP、connectors 和 domain packs 有来源、许可证、版本、完整性、权限 manifest、安装 receipt、enable/disable/update/rollback、冲突诊断和可见性；未知项目资源默认不加载
- [ ] **CORE-14**: usage、token、估算成本、延迟、retry、worker budget、tool/connector 健康、policy denial 和 audit event 可按 session/workflow/provider/tenant 查询；本地遥测默认关闭或仅本地
- [ ] **CORE-15**: Local Personal 无需账户即可运行三个 pack；代码、session、memory、workflow、evidence 默认留在设备；用户可备份、迁移、导出和彻底删除；sync/telemetry 明示 opt-in
- [ ] **CORE-16**: Linux/macOS 原生、Windows/WSL 安装与卸载；升级/回滚/数据迁移；签名、checksum、SBOM、许可证/依赖扫描、release notes、doctor 和支持包；所有声明绑定 release proof

### Coding Pack

- [ ] **COD-01**: 发布一份带冻结日期的 Claude Code public-parity ledger，覆盖安装/登录、CLI flags、交互命令、IDE、Desktop/Web、models、memory、MCP、subagent/team、plugins/skills/hooks、checkpoint、Chrome、Git/CI、SDK、voice 等公开旅程；每行必须有结果证据或明确差异决策
- [ ] **COD-02**: init 一个仓库后可生成/发现项目说明、agents、rules、prompts、checks 和 memory；展示作用域与优先级；未信任项目不加载自动化资源
- [ ] **COD-03**: 浏览目录、Read/Grep/Glob、symbol/reference、repo map、dependency/impact/trace 和图片/截图输入可组合使用；结果显示遗漏、预算和来源
- [ ] **COD-04**: 用户可固定 editable/read-only 文件、引用文件/目录/URL/图片、查看 context 使用量、compact、恢复 compact 后语义，并知道哪些内容被省略
- [ ] **COD-05**: Write/Edit/Delete/NotebookEdit 有精确 patch、AI diff 预览、changed-files 列表、read-before-write/mtime 冲突保护；每个 assistant turn 可 checkpoint、diff、undo，不能覆盖用户后续修改
- [ ] **COD-06**: shell/PowerShell 任务支持 cwd/env、streaming、timeout、cancel、background/daemon、output truncation、sandbox/network 状态和结构化退出；危险命令在执行前被策略拒绝或审批
- [ ] **COD-07**: Git status/diff/log/branch/commit 辅助、worktree 隔离、冲突检测、PR 创建/检查/review、GitHub Actions/CI 触发均保留预览、审批和远端回执；不自动处理用户未授权改动
- [ ] **COD-08**: 自动发现并运行 format/lint/typecheck/build/test；失败形成诊断和受限 repair loop；debug 支持日志、LSP diagnostics、复现步骤和最大重试；通过后再 review
- [ ] **COD-09**: Ask、Plan、Edit/Execute 等模式及 slash command、快捷键、output style、status line、interactive question、permission prompt 都有一致行为；切换模式不丢 session 或绕过 policy
- [ ] **COD-10**: MCP client/server 覆盖 tools/resources/resource templates/prompts 和 stdio/HTTP/SSE/WS；skills/plugins/hooks/commands 可安装、验证、禁用和调试；项目级资源受 trust gate
- [ ] **COD-11**: subagent、agent teams 和并行任务可由用户显式启动或由 router 在授权后启动；每个 worker 有隔离 scope、消息、预算、状态、取消、handoff 和集成 gate
- [ ] **COD-12**: Browser/Chrome、computer-use、screen capture、clipboard、URL handler、Notebook、LSP 等 workbench 可按平台探测、权限启用、暂停和审计；不可用时明确降级
- [ ] **COD-13**: 非交互 print/exec、stream-json、SDK/RPC/MCP 可在 CI 和第三方应用中创建/恢复 session、订阅 events、响应 approval、取消任务并取得 typed result；GitHub Actions 有受限凭据和 PR 回执
- [ ] **COD-14**: 本地任务可转为后台或 remote worker，离线后继续；用户从 CLI、IDE、Desktop、Web 查看同一进度、diff、日志和 approval，重新连接不重复副作用
- [ ] **COD-15**: voice 输入可开始、暂停、编辑确认并提交 prompt；音频权限、转写来源和保留策略可见，不可用时回退文本，不让语音绕过审批
- [ ] **COD-16**: code review、dependency/security review、test coverage 和 release review 产出带文件/行/严重度/置信度/复现与验证证据的 findings；未验证安全问题不得包装成事实

### Academic Research Pack

- [ ] **RES-01**: 研究者可定义 research question、scope、inclusion/exclusion criteria、假设、变量、伦理/数据限制、里程碑和 acceptance；变更形成 decision history
- [ ] **RES-02**: 通过可配置 search/connectors 检索文献、数据集和代码；显示 query、来源、抓取时间、许可/访问状态；合法下载与仅记录 DOI/arXiv/URL 明确区分
- [ ] **RES-03**: PDF、网页、supplement、表格和扫描件可解析、OCR、去重、版本关联；每个 excerpt 保留页码/section/坐标、content hash 和 parser warning
- [ ] **RES-04**: 文献库支持 DOI/arXiv/ISBN/URL 元数据校验、BibTeX/RIS/CSL import/export、引用样式、重复/撤稿/版本提示；正文引用可跳回 source record
- [ ] **RES-05**: notes、claims、counter-evidence、methods、datasets、experiments 和 artifacts 形成 evidence graph；边有 extracted/inferred/ambiguous 和 confidence；孤立 claim 被标记
- [ ] **RES-06**: 系统可生成可编辑 literature review matrix、研究计划、related-work taxonomy 和 progress board；每个 synthesized statement 引用具体来源，冲突证据并列显示
- [ ] **RES-07**: 数据集和代码仓库 intake 记录版本、license、checksum、schema、split、预处理、环境和访问限制；敏感数据有本地/组织 policy
- [ ] **RES-08**: 实验定义参数、seed、环境、输入 hash、代码 revision、资源预算和预期指标；local/remote execution 可暂停恢复；每次 run 产出 immutable result artifact
- [ ] **RES-09**: notebook/data interpreter 在隔离环境运行 Python 等受支持 kernel，支持表格检查、统计检验、effect size、confidence interval、多重比较提示和图表 provenance；不会把失败 cell 当结果
- [ ] **RES-10**: 支持 benchmark、baseline、ablation、error analysis、robustness 和 qualitative case workflow；metric definition、selection decision 和 negative result 均进入 ledger
- [ ] **RES-11**: 论文 workspace 支持大纲、章节、LaTeX/Markdown/Word、figure/table、citation、claim-support 检查、术语一致性和 reviewer-facing diff；任何无来源数字/引用被阻塞或标记
- [ ] **RES-12**: 一键生成但可审查的 reproducibility package：环境 lock、code/data manifest、run commands、results、licenses、limitations、artifact checksums；另生成投稿 checklist、supplement 和 response-to-reviewers 工作区
- [ ] **RES-13**: EDA、硬件、机器人等领域 pack 可注册领域 objects、tools、rules、eval 和 views；EDA 1.0 至少覆盖需求、结构化原理图/网表、BOM/Gerber/CPL/DFM intake、risk review 和 bring-up plan，工程签字/下单保持人工审批
- [ ] **RES-14**: Research verifier 对 DOI、引用、数据、实验、统计、图表和外部投稿状态逐类验证；缺证据输出 blocked/rework/unknown，禁止模型声明使论文结论自动完成

### Daily Work Pack

- [ ] **DAY-01**: 本地文件、笔记、待办、提醒和个人知识库可 create/read/update/search/link/archive/export；每项有来源、时间、状态和冲突处理
- [ ] **DAY-02**: Calendar connector 可查询 availability、创建/改期/取消事件、处理时区/重复规则/参与者；写入前展示变化并记录服务端 event ID
- [ ] **DAY-03**: Email 和消息 connector 支持检索、thread/context、草稿、附件、收件人解析、发送预览、审批、发送回执和失败/未知处理；默认不自动发送
- [ ] **DAY-04**: Word/Markdown/PDF 文档、spreadsheet 和 presentation 可读取、创建、编辑、批注、导出和 diff；公式、图表、引用和版式变化有结构化 warning；不支持元素明确降级
- [ ] **DAY-05**: meeting workflow 支持 agenda、材料包、时间提醒、经授权的录音/转写、speaker/time provenance、notes、decision、action item 和 follow-up draft；参会者隐私策略可见
- [ ] **DAY-06**: browser/desktop automation 可完成受限导航、表单、下载/上传、clipboard 和 app control；每一步有可视状态、目标校验、timeout/cancel、checkpoint/compensation 和 receipt
- [ ] **DAY-07**: 项目/流程模板可把目标拆成 DAG、board、task、owner、deadline、approval 和 report；支持 Coding/Research/Daily 混合 task 和 bounded multi-agent
- [ ] **DAY-08**: workspace search 跨本地文件、notes、mail、calendar、meeting 和获批 connectors，结果按权限裁剪并显示来源/新鲜度；断开 connector 后缓存按 policy 删除或降级
- [ ] **DAY-09**: connector center 展示 discover/connect/OAuth scopes/health/last sync/data access；支持 revoke、re-auth、least privilege、per-workspace enable 和 audit；凭据不进入模型上下文
- [ ] **DAY-10**: approval inbox 汇总待发送、待发布、待删除、待付款、权限变更和结果未知动作；用户可 inspect/edit/approve/deny/escalate；决定同步到原 workflow
- [ ] **DAY-11**: Daily verifier 对目标应用的最终状态、外部 ID、回执、附件 hash、参与者和审批进行核对；无法确认时进入 result_unknown 并禁止自动重放
- [ ] **DAY-12**: 团队空间支持共享 project/task/artifact、comments、mentions、handoff、role visibility 和 audit；个人私有对象默认不因加入团队而共享

### Product Surfaces

- [ ] **SURF-01**: CLI/REPL/TUI 支持 prompt、commands、history/resume、diff、tool cards、approval、background/workflow monitor、settings/doctor、附件和三个 pack；文本与 JSON/stream 输出互不混淆
- [ ] **SURF-02**: Headless SDK/RPC 有版本化 API、typed events、cancel/backpressure/reconnect、approval callback、idempotency、auth 和 language-neutral examples；breaking change 有迁移策略
- [ ] **SURF-03**: MCP server/client 对 tools/resources/templates/prompts、auth、capability discovery、error 和 transports 做完整互操作；policy 与本地工具路径一致
- [ ] **SURF-04**: IDE 至少交付 VS Code 与 JetBrains 的项目上下文、chat、inline/patch diff、diagnostics、review、task/workflow state、approval 和 resume；不得有独立 session/permission model
- [ ] **SURF-05**: Desktop 提供 Claude Desktop-like conversation/workspace、files/sources、artifacts、connector center、model/settings、computer-use、approval inbox、history/search 和三个 pack views；本地 mode 无账户可用
- [ ] **SURF-06**: Web/App Server 提供 conversations/workflows/events/files/artifacts/team/admin/release operations，支持实时 reconnect、RBAC、pagination 和 bounded history；local app server 与 hosted server 使用同一 contract
- [ ] **SURF-07**: 用户可在 CLI 创建 workflow、IDE 查看 diff、Desktop 审批、Web 观察 remote worker；所有端看到相同 IDs、events、state 和 evidence，离线编辑冲突有明确解决
- [ ] **SURF-08**: 所有入口有一致的 loading/empty/error/offline/degraded/permission-denied/result-unknown 状态；长内容、键盘导航、screen reader、reduced motion 和本地化可用
- [ ] **SURF-09**: Linux/macOS 原生和 Windows/WSL 的路径、terminal、sandbox、Keychain、browser/native host 差异被端到端测试；unsupported capability 明示而非隐藏
- [ ] **SURF-10**: 每个入口可查看 capability/readiness、provider/connector/plugin health、policy、data location、version/update、release blockers 和支持诊断；输出脱敏且可导出

### Official Cloud

- [ ] **CLOUD-01**: 可选账户支持注册/登录/OAuth、MFA、session/device 管理、recovery、注销和账号删除；未登录仍可使用完整 Local Personal
- [ ] **CLOUD-02**: 用户显式选择的 session/workflow/artifact/memory 可加密同步；显示同步范围、方向、冲突、last sync、设备和恢复状态；可暂停、导出、删除云端副本
- [ ] **CLOUD-03**: remote worker 接收签名 WorkPacket，在隔离 workspace 执行，stream typed events，支持 reconnect/cancel/timeout/budget；结果经 verifier 后回传，未知副作用不重放
- [ ] **CLOUD-04**: team workspace 支持成员/邀请、共享 project/workflow/artifact、comments/mentions、handoff、approval 和 activity；个人与团队数据边界显式
- [ ] **CLOUD-05**: tenant isolation 覆盖 storage、cache、queue、logs、search index、worker、connector credentials 和 support tooling；跨租户访问有自动化负面测试
- [ ] **CLOUD-06**: subscription、plan、quota、usage、invoice、payment failure、trial/cancel 和 entitlement 有可审计 ledger；计费失败不删除本地数据或锁死导出
- [ ] **CLOUD-07**: 用户可设置 retention/residency、下载 portable export、删除 workspace/account 数据并取得 deletion status；备份与恢复策略明确，支持密钥轮换
- [ ] **CLOUD-08**: 服务提供 status/health、SLO、rate limit、queue visibility、incident/audit、backup restore、abuse control 和 support diagnostics；降级时本地工作继续

### Enterprise Self-hosted

- [ ] **ENT-01**: 提供联网和 air-gapped 安装 bundle、环境 preflight、容量规划、校验和/SBOM、离线依赖、upgrade、rollback、uninstall 和数据 migration；失败可恢复到已验证版本
- [ ] **ENT-02**: 支持 OIDC/SAML SSO、MFA policy、local break-glass admin、用户停用和组织/团队映射；认证故障有安全降级
- [ ] **ENT-03**: RBAC 至少区分 user、workspace admin、security/policy admin、auditor、operator；可对 provider、model、tool、connector、plugin、data、remote worker 和高风险动作设置集中 policy，deny 优先
- [ ] **ENT-04**: secrets 与 customer-managed keys 接入 vault/HSM/KMS，支持 scope、rotation、revocation、audit 和 zero-secret diagnostic；worker 只获得短期最小凭据
- [ ] **ENT-05**: append-only audit 覆盖 login、policy、approval、tool/connector、data access、admin、export 和 release；支持检索、导出、retention、legal hold 和完整性验证
- [ ] **ENT-06**: 支持 outbound allowlist/proxy/custom CA、offline model/provider、private MCP/connectors、artifact quarantine 和 egress review；受限网络下 status 能解释缺失能力
- [ ] **ENT-07**: 数据 retention/residency、workspace export/delete、backup/restore、disaster recovery、schema migration 和 integrity check 有 runbook 与演练证据；恢复保留 event/evidence 因果顺序
- [ ] **ENT-08**: 提供 logs/metrics/traces、health/readiness、queue/worker/provider/connector dashboard、alert、support bundle 和脱敏 remote support 流程；可接常见 observability stack
- [ ] **ENT-09**: license/entitlement 支持离线签发、到期宽限、续期、席位/容量核对和可审计状态；许可证问题不能破坏客户数据访问、导出或恢复
- [ ] **ENT-10**: 发布企业支持矩阵、security/privacy/licensing 文档、漏洞报告与修复 SLA、管理员/用户/API 手册、迁移与破坏性变更策略；目标客户验收有签字 evidence

### Product Differentiators

- [ ] **DIF-01**: Evidence-first completion 让用户展开每次 `complete` 的 acceptance 与逐条 evidence；模型自述永远不能单独完成任务
- [ ] **DIF-02**: Durable long-task recovery 让任务在 crash、断网或切换入口后恢复到最后可信节点；EventLog 可重建 state，partial/unknown 不冒充 Done
- [ ] **DIF-03**: Result-unknown side-effect safety 让邮件、付款、发布、上传等响应丢失时先查询目标系统或请求人工核对，绝不盲目重放
- [ ] **DIF-04**: Bounded multi-agent with integration proof 让每个 worker 的 scope、预算、路径、工具、结果和失败可查看；冲突被阻塞，集成后整体 gate 重跑
- [ ] **DIF-05**: Local-first full product without account 让个人用户离线运行 Core 与三个 pack 并拥有 session、memory、workflow 和 evidence；云只增加同步、远程与团队规模
- [ ] **DIF-06**: One state model across three packs and all surfaces 让同一 workflow 组合代码、实验和办公审批，且 CLI、IDE、Desktop、Web 看到同一 event/evidence
- [ ] **DIF-07**: Explainable provider capability negotiation 让每次模型选择显示原因、能力差异、fallback 和质量/成本影响；不支持的工具或 vision 不会静默失败
- [ ] **DIF-08**: Provenance-aware context and memory 让每段上下文、记忆、claim 和 graph edge 可回到 source/hash/time/confidence，并明显降权 stale 或 inferred 内容
- [ ] **DIF-09**: Autonomy profiles with immutable hard boundaries 允许用户切换安全、平衡和自治档位，但删除、发布、付款、凭据和外部消息等硬策略始终生效
- [ ] **DIF-10**: Verifiable Research integrity 让引用、数据、实验、统计和图表进入专用 verifier；无法验证的论文 claim 被阻塞而不是被润色掩盖
- [ ] **DIF-11**: Audited capability superset 让 38 个 reference 和专有公开基线逐项记录 Adopt/Adapt/Reject、license、安全、owner、test 与 evidence，并能解释取舍
- [ ] **DIF-12**: Transparent release and enterprise readiness 让用户和管理员查看 local/external blockers、签名、SBOM、平台与 acceptance 证据，未完成能力不能被营销措辞掩盖

## v2 Requirements

以下能力明确推迟到 1.0 之后，不得用来替代或推迟已批准的 v1 能力。

### Post-1.0 Extensions

- **POST-01**: 原生 Windows 客户端与完整 Windows sandbox **Deferred because:** 已批准 1.0 通过 Windows/WSL 支持；原生实现需要独立安全、terminal、Keychain、native-host 和安装矩阵 **Prerequisite:** SURF-09、CORE-16 的 WSL 旅程稳定
- **POST-02**: 原生 iOS/Android 客户端 **Deferred because:** 1.0 可由响应式 Web 完成观察/审批；原生移动端会引入另一套发布与安全生命周期 **Prerequisite:** SURF-06、SURF-07、Cloud 稳定
- **POST-03**: 多区域 active-active、跨云调度和极端规模 worker fleet **Deferred because:** 1.0 需要可靠 official cloud，不需要先解决全球超大规模 **Prerequisite:** CLOUD-03、CLOUD-05、CLOUD-08 有真实负载数据
- **POST-04**: P2P / local-only 多设备同步 **Deferred because:** 有价值但冲突、密钥发现和网络穿透复杂；不应阻塞 1.0 的显式云同步 **Prerequisite:** CLOUD-02 同步语义稳定
- **POST-05**: Marketplace 付费结算、分成和公开评级体系 **Deferred because:** 1.0 需要可信扩展生命周期，不需要先商业化第三方生态 **Prerequisite:** CORE-13 签名、review、rollback、abuse 流程成熟
- **POST-06**: 用户自训练/fine-tune 模型与训练集管理 **Deferred because:** Kiana 1.0 是 Agent 平台而非 foundation-model 训练平台；先把 provider 和 eval 做正确 **Prerequisite:** CORE-03、CORE-14、pack eval 稳定
- **POST-07**: 实时双向语音/视频会议 Agent 与 ambient always-on assistant **Deferred because:** 1.0 的 voice prompt 和经授权 meeting capture 足够；常驻监听有额外隐私和资源风险 **Prerequisite:** COD-15、DAY-05、隐私评测成熟
- **POST-08**: 新增法律、医疗、财务等高风险一方领域包 **Deferred because:** 1.0 已承诺 Research 与 EDA/硬件/机器人扩展；新高风险领域需要单独责任与监管设计 **Prerequisite:** domain-pack contract、领域 verifier、责任边界成熟
- **POST-09**: 专业 CAD/EDA 自动布局布线、SI/PI/热仿真和自动生产输出 **Deferred because:** 应优先连接专业工具并保留工程师 gate；自研专业求解器会偏离核心产品 **Prerequisite:** RES-13 connector 与 evidence 工作流稳定
- **POST-10**: 超大规模开放式 agent society / 自由群聊 **Deferred because:** 对用户价值和可靠性未证明，且与 bounded execution 原则冲突；仅在受限研究模式评估 **Prerequisite:** CORE-12 真实数据证明需要
- **POST-11**: 沉浸式 3D/AR 工作区和装饰性知识宫殿 UI **Deferred because:** 不改善当前核心任务完成率，且可能掩盖 provenance 与密集工作流 **Prerequisite:** Desktop/Web 基本工作台成熟
- **POST-12**: 完全自治的 release/采购/付款/组织权限变更 **Deferred because:** 即使未来也只能在组织明确双人审批和可逆边界内增强；“无人批准”本身不是目标 **Prerequisite:** 不能取消 AF-08 的硬边界

## Out of Scope

以下 anti-features 明确排除，用于防止许可证、安全、数据、可靠性和完成证明边界被重新引入。

| Feature | Reason | Kiana Alternative |
|---------|--------|-------------------|
| **AF-01**: 复制 Claude Code、Claude Desktop 或其他专有产品的源码、品牌、素材或隐藏协议 | 侵权、不可维护、无法独立演进 | 仅冻结公开可观察行为，clean-room 独立实现并保留差异证据 |
| **AF-02**: 机械合并全部 reference 代码 | 许可证冲突、重复 runtime、不同安全模型、stubs 和依赖爆炸 | 每项 Adopt/Adapt/Reject；代码复用先过 license、架构、安全和测试 gate |
| **AF-03**: 用命令、页面、类型、模块、测试桩或 mock response 数量证明完成 | 产生 completion theater，真实旅程仍不可用 | requirement → journey → test → evidence → release proof 的闭环 |
| **AF-04**: 为 Desktop、Web、IDE、Cloud 或每个 pack 复制模型 loop、session、permission、workflow 或 event schema | 状态分叉、策略绕过、跨端无法恢复、长期重复维护 | 所有入口/pack 只扩展 shared Core contract |
| **AF-05**: 强制账户、云同步或订阅才能使用本地 Core/pack | 违背 Local-first，制造数据与供应商锁定 | 本地完整；只对同步、remote、team scale 和 enterprise governance 收费 |
| **AF-06**: 把 API key、OAuth token、Cookie、SSH key、license key 写入项目、普通日志、session 或模型上下文 | 凭据泄露与供应链攻击 | Keychain/vault、短期 token、脱敏 metadata、zero-secret diagnostics |
| **AF-07**: 自治模式、hook、plugin、MCP、managed override 或 remote worker 绕过 ProjectTrust、sandbox、network/exec policy 或硬审批 | 任意代码执行、数据外泄和不可逆损失 | autonomy 只能在 hard policy 内增加预算与自动化程度 |
| **AF-08**: 自动执行或重放删除、发送、发布、付款、采购、权限变化、push/merge/deploy 等高风险动作而无明确策略/审批/回执 | 目标错误、重复副作用、法律和财务风险 | preview + scoped approval + idempotency + receipt + result_unknown query/人工核对 |
| **AF-09**: 生成不存在的 DOI/引用/数据/实验/显著性/图表/审稿状态或任务完成状态 | 学术不端和错误决策 | provenance verifier；无证据时 rework/blocked/unknown |
| **AF-10**: 无界 agent loop、无限 worker、自由 speaker 群聊或无预算 retry | 成本失控、冲突、重复失败和无法归责 | bounded WorkPacket、max workers、budget、termination、path lock、integration gate |
| **AF-11**: 让 memory、embedding 或 knowledge graph 覆盖 live file、git、test、外部状态或原始文献 | stale/inferred 数据被当事实 | source priority、freshness、confidence、live refresh 和可点击原始证据 |
| **AF-12**: 未签名、无 provenance/permission manifest 的 marketplace 自动安装或自动更新 | 供应链和权限升级攻击 | 来源/版本/hash/signature/receipt/policy/review/rollback |
| **AF-13**: 隐藏 telemetry、默认上传代码/session/memory，或用模糊文案捆绑 sync | 隐私、信任和企业合规失败 | 明示 opt-in、数据预览、按类开关、retention/export/delete |
| **AF-14**: 把不同 provider 的 tool、vision、context、reasoning 或 structured output 当作完全等价 | 任务在弱能力模型上静默降质或失败 | capability negotiation、routing、visible fallback、明确拒绝 |
| **AF-15**: 默认启动攻击性 pentest、漏洞利用、凭据测试或外部扫描 | 越权、法律和生产风险 | 默认 defensive review；攻击性操作需明确授权、scope、隔离和证据留存 |
| **AF-16**: 自动 commit 用户全部 dirty state、reset/checkout、清理 untracked 或覆盖 agent 结束后的用户修改 | 数据丢失和归属不明 | touch-set、checkpoint、isolated worktree、late-edit conflict、明确批准 |
| **AF-17**: 让 Research/EDA Agent 替代专业工程师签字、自动下单或承诺器件供应/安全 | 工程责任、实时供应链和人身安全风险 | 结构化 review、风险提示、专业工具 connector、人工 approval/sign-off |
| **AF-18**: 为追求像素级/命令级兼容而冻结 Kiana 自己的 UX 和配置模型 | 绑定专有产品演进并妨碍统一三 pack 架构 | 能力与真实 journey 对齐，提供必要 migration/import 而非永久克隆 |

## Acceptance Criteria

- 所有 v1 requirement 均映射到且只映射到一个 roadmap phase，并绑定 owner、实现位置、测试、evidence 和当前 proof level。
- Coding、Academic Research 和 Daily Work 均完成版本化黄金旅程，且同一 workflow 可跨能力包组合而不复制状态。
- CLI/TUI、Headless SDK/RPC/MCP、IDE、Desktop 和 Web/App Server 使用一致的 IDs、events、policy、state、approval 和 evidence。
- Local Personal 在无账户和无官方云连接时可完整运行三个能力包；云和企业能力是显式启用的适配层。
- 所有 Provider 对 tool use、vision、structured output、reasoning、streaming 和 context 能力进行显式协商、路由、降级或拒绝。
- 崩溃、断网、worker 丢失、projection 损坏和外部响应丢失均有可验证恢复路径；未知外部结果不得盲目重放。
- 安全、平衡和自治档位都服从不可绕过的 ProjectTrust、组织 deny、sandbox、network、secret 和高风险副作用策略。
- Research 引用、数据、实验、统计、图表和论文 claim 均可回溯到 source/artifact/hash；缺证据时必须 blocked、rework 或 unknown。
- Cloud 与 Enterprise 的 tenant scope 覆盖数据库、对象、队列、缓存、索引、vault、worker、日志和支持工具，并通过跨租户负面测试。
- Linux/macOS 原生及 Windows/WSL 的安装、升级、回滚、卸载、数据迁移和平台能力差异通过目标环境验证。
- 38/38 reference 治理表保持 live source、license、Adopt/Adapt/Reject、owner、test、risk 和 evidence 可追踪。
- `local_blocking`、`external_blocking`、P0/P1、签名、SBOM、供应链、运维和目标用户验收门禁全部满足后才能发布 1.0。

## Definition of Done

Kiana 1.0 只有在以下条件同时成立时才完成：

1. 98 项 v1 requirements 全部实现、验证并提交，roadmap traceability 覆盖率为 100%。
2. 三个能力包、全部产品入口、Local Personal、Official Cloud 和 Enterprise Self-hosted 均通过各自目标环境黄金旅程。
3. 所有高风险副作用、恢复、租户隔离、Research integrity 和跨入口连续性均通过故障注入与负面测试。
4. 发布物可复现、签名、带 checksum/SBOM/provenance，并完成 Linux/macOS/Windows-WSL 生命周期验证。
5. 不存在未处置的 P0/P1 缺陷、已知数据损坏路径、可绕过策略或以 mock/local proof 冒充目标证明的条目。
6. 文档、迁移、管理员、API、支持、安全响应、备份恢复和商业运维流程均有 owner 与验收证据。
7. 只有在 local 与 external blockers 均归零并取得目标用户/客户验收后，版本才可标记为 `1.0`、`complete` 或 `production-ready`。

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| CORE-01 | Phase 3 | Pending |
| CORE-02 | Phase 4 | Pending |
| CORE-03 | Phase 7 | Pending |
| CORE-04 | Phase 3 | Pending |
| CORE-05 | Phase 5 | Pending |
| CORE-06 | Phase 5 | Pending |
| CORE-07 | Phase 6 | Pending |
| CORE-08 | Phase 4 | Pending |
| CORE-09 | Phase 8 | Pending |
| CORE-10 | Phase 8 | Pending |
| CORE-11 | Phase 8 | Pending |
| CORE-12 | Phase 9 | Pending |
| CORE-13 | Phase 9 | Pending |
| CORE-14 | Phase 7 | Pending |
| CORE-15 | Phase 19 | Pending |
| CORE-16 | Phase 24 | Pending |
| COD-01 | Phase 1 | Pending |
| COD-02 | Phase 10 | Pending |
| COD-03 | Phase 10 | Pending |
| COD-04 | Phase 10 | Pending |
| COD-05 | Phase 10 | Pending |
| COD-06 | Phase 10 | Pending |
| COD-07 | Phase 10 | Pending |
| COD-08 | Phase 10 | Pending |
| COD-09 | Phase 10 | Pending |
| COD-10 | Phase 11 | Pending |
| COD-11 | Phase 11 | Pending |
| COD-12 | Phase 11 | Pending |
| COD-13 | Phase 11 | Pending |
| COD-14 | Phase 11 | Pending |
| COD-15 | Phase 11 | Pending |
| COD-16 | Phase 10 | Pending |
| RES-01 | Phase 12 | Pending |
| RES-02 | Phase 12 | Pending |
| RES-03 | Phase 12 | Pending |
| RES-04 | Phase 12 | Pending |
| RES-05 | Phase 12 | Pending |
| RES-06 | Phase 12 | Pending |
| RES-07 | Phase 13 | Pending |
| RES-08 | Phase 13 | Pending |
| RES-09 | Phase 13 | Pending |
| RES-10 | Phase 13 | Pending |
| RES-11 | Phase 13 | Pending |
| RES-12 | Phase 13 | Pending |
| RES-13 | Phase 13 | Pending |
| RES-14 | Phase 13 | Pending |
| DAY-01 | Phase 14 | Pending |
| DAY-02 | Phase 14 | Pending |
| DAY-03 | Phase 14 | Pending |
| DAY-04 | Phase 14 | Pending |
| DAY-05 | Phase 14 | Pending |
| DAY-06 | Phase 15 | Pending |
| DAY-07 | Phase 15 | Pending |
| DAY-08 | Phase 14 | Pending |
| DAY-09 | Phase 14 | Pending |
| DAY-10 | Phase 14 | Pending |
| DAY-11 | Phase 15 | Pending |
| DAY-12 | Phase 15 | Pending |
| SURF-01 | Phase 16 | Pending |
| SURF-02 | Phase 16 | Pending |
| SURF-03 | Phase 16 | Pending |
| SURF-04 | Phase 17 | Pending |
| SURF-05 | Phase 18 | Pending |
| SURF-06 | Phase 18 | Pending |
| SURF-07 | Phase 19 | Pending |
| SURF-08 | Phase 18 | Pending |
| SURF-09 | Phase 19 | Pending |
| SURF-10 | Phase 16 | Pending |
| CLOUD-01 | Phase 20 | Pending |
| CLOUD-02 | Phase 20 | Pending |
| CLOUD-03 | Phase 21 | Pending |
| CLOUD-04 | Phase 21 | Pending |
| CLOUD-05 | Phase 20 | Pending |
| CLOUD-06 | Phase 21 | Pending |
| CLOUD-07 | Phase 20 | Pending |
| CLOUD-08 | Phase 21 | Pending |
| ENT-01 | Phase 22 | Pending |
| ENT-02 | Phase 22 | Pending |
| ENT-03 | Phase 22 | Pending |
| ENT-04 | Phase 22 | Pending |
| ENT-05 | Phase 23 | Pending |
| ENT-06 | Phase 22 | Pending |
| ENT-07 | Phase 23 | Pending |
| ENT-08 | Phase 23 | Pending |
| ENT-09 | Phase 22 | Pending |
| ENT-10 | Phase 23 | Pending |
| DIF-01 | Phase 8 | Pending |
| DIF-02 | Phase 8 | Pending |
| DIF-03 | Phase 8 | Pending |
| DIF-04 | Phase 9 | Pending |
| DIF-05 | Phase 19 | Pending |
| DIF-06 | Phase 19 | Pending |
| DIF-07 | Phase 7 | Pending |
| DIF-08 | Phase 12 | Pending |
| DIF-09 | Phase 5 | Pending |
| DIF-10 | Phase 13 | Pending |
| DIF-11 | Phase 1 | Pending |
| DIF-12 | Phase 2 | Pending |

**Coverage:**
- v1 requirements: 98 total
- Mapped to phases: 98
- Unmapped: 0

---
*Requirements defined: 2026-07-15*
*Last updated: 2026-07-15 after initial definition*
