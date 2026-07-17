# Roadmap: Kiana

## Overview

Kiana 1.0 沿着“证据治理 -> 契约、状态、策略与运行时 -> 可靠执行 -> 三个能力包 -> 全部产品入口 -> Official Cloud 与 Enterprise -> 发布证明”的依赖链演进。Coding、Academic Research 与 Daily Work 在共同基础完成后可并行推进，Coding 获得最高投入，但三个能力包、所有入口、Local Personal、Official Cloud、Enterprise Self-hosted 和 12 项差异化要求都必须在 1.0 收敛，不因排序而延期。

## Phases

**Phase Numbering:**

- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions marked as INSERTED

- [ ] **Phase 1: 现状基线与证据治理** - 冻结公开行为和 38-reference 的可审计基线。
- [ ] **Phase 2: 可复现工具链与依赖收敛** - 让构建输入和发布就绪状态可复现、可检查。
- [ ] **Phase 3: 契约与 Schema 基线** - 统一事件、工具和跨入口协议语义。
- [ ] **Phase 4: 状态权威与投影恢复** - 建立可迁移、可重放、可重建的持久状态。
- [ ] **Phase 5: 策略、信任、凭据与本地数据边界** - 让所有入口共享不可绕过的安全决策。
- [ ] **Phase 6: RuntimeHost 与运行时抽取** - 让每个入口获得一致且可追溯的执行上下文。
- [ ] **Phase 7: Provider 能力协商与适配闭环** - 显式呈现多 Provider 的路由、降级和成本差异。
- [ ] **Phase 8: 可靠 Workflow、证据与副作用语义** - 用验收、证据、恢复和 result_unknown 驱动完成状态。
- [ ] **Phase 9: 受限多 Agent、扩展与 Pack 契约** - 让并行执行和扩展生态可隔离、可审计、可回滚。
- [ ] **Phase 10: Coding 公开基线与仓库闭环** - 完成从项目理解到安全修改、验证和 review 的真实旅程。
- [ ] **Phase 11: Coding 生态、自动化与远程闭环** - 完成 MCP、Agent、workbench、Headless、远程和语音旅程。
- [ ] **Phase 12: Research 来源、引用与证据图谱** - 建立从研究问题到可定位 claim 的可信来源链。
- [ ] **Phase 13: Research 实验、论文、复现与领域 Pack** - 完成可验证实验、论文和领域研究交付。
- [ ] **Phase 14: Daily 对象、Connector 与审批控制** - 建立个人对象、办公连接器和统一审批入口。
- [ ] **Phase 15: Daily 自动化、回执与团队 Handoff** - 安全完成跨应用自动化、混合流程和团队交接。
- [ ] **Phase 16: Terminal、Headless 与 MCP 产品闭环** - 让最早期产品入口完整消费统一核心。
- [ ] **Phase 17: IDE 客户端** - 交付共享状态模型的 VS Code 与 JetBrains 体验。
- [ ] **Phase 18: Desktop 与 Web/App Server** - 交付本地桌面工作区和统一 Web 控制面。
- [ ] **Phase 19: 跨入口本地连续性与平台交付** - 证明无账户本地产品在多端和目标平台连续工作。
- [ ] **Phase 20: Official Cloud 身份、同步与租户数据基础** - 建立可选账户、加密同步和租户隔离。
- [ ] **Phase 21: Official Cloud Worker、团队、计费与运维** - 完成可商业运营的远程执行与协作服务。
- [ ] **Phase 22: Enterprise 部署、身份与集中治理** - 完成自托管安装、企业身份、RBAC、凭据和受限网络。
- [ ] **Phase 23: Enterprise 审计、数据生命周期、DR 与支持** - 完成企业恢复、运维和支持闭环。
- [ ] **Phase 24: 1.0 全量收敛与发布证明** - 仅用真实目标环境和用户验收授权 1.0 发布。

## Phase Details

### Phase 1: 现状基线与证据治理

**Goal**: 用户和维护者可以用冻结日期、来源和证据判断公开能力与 reference 覆盖，而不是依赖功能数量或乐观描述。
**Depends on**: Nothing (first phase)
**Requirements**: COD-01, DIF-11
**Success Criteria** (what must be TRUE):

  1. 用户可以查看带冻结日期的 Claude Code public-parity ledger，并为每个公开旅程找到验证结果或明确差异决策。
  2. 维护者可以检查 38/38 reference 的 live source、license、Adopt/Adapt/Reject、owner、test、risk 与 evidence，且任何拒绝都有理由。
  3. 进度与审计报告能区分 source、local、target 和 user-value proof，不会把模块、stub、mock 或测试数量报告成产品完成。

**Plans**: 8/12 plans executed

**Wave 1**

- [x] 01-01-PLAN.md
- [x] 01-02-PLAN.md

**Wave 2** *(blocked on Wave 1 completion)*

- [x] 01-03-PLAN.md
- [x] 01-04-PLAN.md
- [x] 01-05-PLAN.md

**Wave 3** *(blocked on Wave 2 completion)*

- [x] 01-06-PLAN.md

**Wave 4** *(blocked on Wave 3 completion)*

- [x] 01-07-PLAN.md

**Wave 5** *(blocked on Wave 4 completion)*

- [x] 01-08-PLAN.md

**Wave 6** *(blocked on Wave 5 completion)*

- [ ] 01-09-PLAN.md

**Wave 7** *(blocked on Wave 6 completion)*

- [ ] 01-10-PLAN.md

**Wave 8** *(blocked on Wave 7 completion)*

- [ ] 01-11-PLAN.md

**Wave 9** *(blocked on Wave 8 completion)*

- [ ] 01-12-PLAN.md

### Phase 2: 可复现工具链与依赖收敛

**Goal**: 用户和发布维护者可以复现构建并透明判断产品离发布就绪还缺什么。
**Depends on**: Phase 1
**Requirements**: DIF-12
**Success Criteria** (what must be TRUE):

  1. 发布维护者可以从固定的工具链、lockfile 和依赖策略复现同一构建输入，并解释依赖或工具版本差异。
  2. 用户和管理员可以查看并导出 local/external blockers、平台、签名、SBOM、license 与 acceptance readiness，敏感信息保持脱敏。
  3. 缺少目标环境或用户证据的能力会保持阻塞状态，不能被标记为 complete、production-ready 或 1.0。

**Plans**: TBD

### Phase 3: 契约与 Schema 基线

**Goal**: 所有客户端以兼容、版本化的事件和 registry 契约理解同一运行事实。
**Depends on**: Phase 1, Phase 2
**Requirements**: CORE-01, CORE-04
**Success Criteria** (what must be TRUE):

  1. CLI、SDK/RPC、MCP、remote/bridge 和 App Server 可以序列化并重放同一组 typed RuntimeEvent、ID、terminal status 与结构化错误。
  2. 旧客户端或旧数据可通过兼容 adapter 迁移；无法兼容的输入会被明确拒绝并给出诊断，而不是静默丢字段。
  3. 用户通过任一入口发现 command、tool、MCP workbench 或 connector 时，看到一致的 schema、版本、权限、生命周期和错误语义。
  4. 并发调用遵循同一 registry 决策：安全读操作可并行，写操作被串行化或隔离，结果在事件流中可观察。

**Plans**: TBD

### Phase 4: 状态权威与投影恢复

**Goal**: 用户的 session 与 memory 在重启、迁移或投影损坏后仍能从可信事实恢复。
**Depends on**: Phase 3
**Requirements**: CORE-02, CORE-08
**Success Criteria** (what must be TRUE):

  1. 用户可以 create、list、resume、fork、compact、import、export 和 delete session，重启后父子 turn、tool lifecycle、附件与 pack 选择保持一致。
  2. 删除或损坏 rebuildable projection 后，系统可以从 authoritative EventLog 重建相同状态；日志完整性无法验证时任务进入 blocked。
  3. 旧 session 有可验证的迁移、回滚与 portable export 路径，不会在升级时被静默丢弃。
  4. 用户可以搜索、编辑、导出和删除带 source、confidence、scope、retention 与 stale 状态的 memory，live evidence 始终优先。

**Plans**: TBD

### Phase 5: 策略、信任、凭据与本地数据边界

**Goal**: 用户可以理解和控制 Kiana 的权限，同时任何自主权档位都服从统一硬边界。
**Depends on**: Phase 3, Phase 4
**Requirements**: CORE-05, CORE-06, DIF-09
**Success Criteria** (what must be TRUE):

  1. 首次启动时用户可以选择安全、平衡或自治档位并按项目覆盖，界面清楚展示实际生效档位与不可绕过的硬策略。
  2. CLI、MCP、remote、Desktop 和 Web 对同一 path、network、exec、sandbox 或外部副作用请求产出一致、可解释的 PolicyDecision，deny 始终优先。
  3. API key、OAuth、Cookie、SSH key 和 license key 只进入 Keychain 或 vault；状态与支持输出仅显示脱敏元数据，安全存储不可用时 fail closed。
  4. 用户可以检查 defaults、user、project、local、env、CLI 与 managed policy 的确定性优先级，并知道某项配置为何生效。

**Plans**: TBD
**UI hint**: yes

### Phase 6: RuntimeHost 与运行时抽取

**Goal**: 用户从任一入口执行任务时都获得同一份预算受控、来源可追溯的上下文。
**Depends on**: Phase 3, Phase 4, Phase 5
**Requirements**: CORE-07
**Success Criteria** (what must be TRUE):

  1. 同一请求经 InProcessHost 或其他 RuntimeHost 入口执行时，客户端可以看到语义一致的 ContextPack 及其 source、time、hash、permission 和 truncation reason。
  2. 用户可以组合 repo map、symbol/path search、impact/trace、editable/read-only file set 与 artifact graph，并检查遗漏项和预算消耗。
  3. session 恢复、入口切换或 retry 不会静默改变已冻结的上下文；必要刷新会生成可观察的新 snapshot 和原因。

**Plans**: TBD

### Phase 7: Provider 能力协商与适配闭环

**Goal**: 用户可以可靠选择多 Provider，并在执行前理解真实能力、fallback 和成本影响。
**Depends on**: Phase 6
**Requirements**: CORE-03, CORE-14, DIF-07
**Success Criteria** (what must be TRUE):

  1. 用户可以注册、认证、选择和健康检查 Anthropic、OpenAI、Gemini、OpenRouter、OpenAI-compatible 与本地模型。
  2. 每次模型选择都会展示 capability source、tool/vision/structured-output/reasoning/streaming/context 支持、选择理由和质量或成本影响。
  3. Provider 不支持任务所需能力时，系统会在调用前显式 route、degrade、emulate 或 reject，并向所有客户端发送同一 capability event。
  4. 用户和管理员可以按 session、workflow、provider 或 tenant 查询 usage、token、估算成本、延迟、retry、budget 与 health，而本地遥测默认不外发。

**Plans**: TBD
**UI hint**: yes

### Phase 8: 可靠 Workflow、证据与副作用语义

**Goal**: 用户只在验收证据成立时看到任务完成，并能在故障或未知外部结果后安全继续。
**Depends on**: Phase 4, Phase 5, Phase 6, Phase 7
**Requirements**: CORE-09, CORE-10, CORE-11, DIF-01, DIF-02, DIF-03
**Success Criteria** (what must be TRUE):

  1. 跨步、跨会话、多 Agent 或有外部副作用的请求会创建持久 WorkflowRun DAG；用户可以检查 router 的 pack、风险、权限、副作用与验收理由。
  2. 每个 complete 状态都可展开 acceptance 与逐条 diff、命令、测试、引用、实验、审批、回执或 artifact evidence；模型自述不能单独完成任务。
  3. crash、interrupt、超时、provider 断流、worker 失败或 projection 损坏后，任务可以恢复到最后可信节点，partial 或 unknown 不会显示为 Done。
  4. 外部响应在 dispatch 后丢失时，任务进入 result_unknown 并先查询目标系统或请求人工核对，绝不盲目重放。
  5. 用户在所有入口看到一致的 complete、rework、blocked、awaiting_approval 与 result_unknown 状态及恢复建议。

**Plans**: TBD
**UI hint**: yes

### Phase 9: 受限多 Agent、扩展与 Pack 契约

**Goal**: 用户可以安全并行执行和扩展 Kiana，并清楚检查每个 worker 与扩展的边界和结果。
**Depends on**: Phase 5, Phase 8
**Requirements**: CORE-12, CORE-13, DIF-04
**Success Criteria** (what must be TRUE):

  1. 用户可以查看每个 worker 的 WorkPacket、允许/禁止路径、tools、预算、依赖、lease、状态、取消、结果和失败；越界或冲突会被阻塞。
  2. 失联或过期 worker 的 late result 不会自动集成，父 workflow 只在重新验证 packet 和整体 gate 后完成。
  3. skills、plugins、hooks、MCP、connectors 与 domain packs 显示来源、license、版本、完整性、权限和 receipt，并支持 enable、disable、update、rollback 与冲突诊断。
  4. 未信任项目资源默认不加载，任何 pack 或扩展都不能建立第二套 session、policy、event 或 completion 模型。

**Plans**: TBD
**UI hint**: yes

### Phase 10: Coding 公开基线与仓库闭环

**Goal**: 开发者可以在真实仓库完成可逆的理解、修改、验证和 review 闭环。
**Depends on**: Phase 9
**Requirements**: COD-02, COD-03, COD-04, COD-05, COD-06, COD-07, COD-08, COD-09, COD-16
**Success Criteria** (what must be TRUE):

  1. 用户 init 或打开仓库后可以发现有作用域和优先级的项目说明、agents、rules、prompts、checks 与 memory；未信任项目不会加载自动化资源。
  2. 用户可以组合目录浏览、Read/Grep/Glob、symbol/reference、repo map、impact/trace、URL、图片与受控 context，并检查来源、遗漏和预算。
  3. Write、Edit、Delete 与 NotebookEdit 提供精确 patch、diff、changed-files、mtime 冲突保护、checkpoint 和 undo，且不会覆盖用户后续修改。
  4. Shell、Git/worktree、format、lint、typecheck、build、test、debug、PR/CI 与受限 repair loop 产生可复现证据和远端回执，不会擅自处理用户未授权 dirty state。
  5. Ask、Plan、Edit/Execute、commands、快捷键、permission prompt 与 review findings 在切换模式后保持同一 session 和 policy，并提供可定位的严重度、置信度与验证证据。

**Plans**: TBD
**UI hint**: yes

### Phase 11: Coding 生态、自动化与远程闭环

**Goal**: 开发者可以通过扩展、自动化、workbench 和远程执行完成 Coding 公共能力旅程。
**Depends on**: Phase 9, Phase 10
**Requirements**: COD-10, COD-11, COD-12, COD-13, COD-14, COD-15
**Success Criteria** (what must be TRUE):

  1. 用户可以通过完整 MCP transports 和 tools/resources/templates/prompts 使用、验证和调试 MCP、skills、plugins、hooks 与 commands，项目资源仍受 trust gate。
  2. 用户可以启动、观察、取消和 handoff subagent 或 agent team，并按平台启用 Browser/Chrome、computer-use、screen capture、clipboard、URL handler、Notebook 与 LSP；不可用能力明确降级。
  3. Headless print/exec、stream-json、SDK/RPC/MCP 与 GitHub Actions 可以创建或恢复 session、订阅 events、响应 approval、取消任务并取得 typed result 与受限凭据回执。
  4. 本地任务可转为 background 或 remote worker；CLI、IDE、Desktop 和 Web 观察同一进度、diff、日志与 approval，重连不会重复副作用。
  5. 用户可以开始、暂停、编辑确认并提交 voice prompt，查看音频权限、转写来源和保留策略，并在不可用时安全回退文本。

**Plans**: TBD
**UI hint**: yes

### Phase 12: Research 来源、引用与证据图谱

**Goal**: 研究者可以从明确问题推进到每条综合结论均可回到原始来源的证据图谱。
**Depends on**: Phase 9
**Requirements**: RES-01, RES-02, RES-03, RES-04, RES-05, RES-06, DIF-08
**Success Criteria** (what must be TRUE):

  1. 研究者可以定义 question、scope、纳排标准、假设、变量、伦理/数据限制、里程碑与 acceptance，并查看 decision history。
  2. 文献、数据集和代码检索显示 query、来源、抓取时间、license/access 状态，并明确区分合法下载与仅记录 DOI、arXiv 或 URL。
  3. PDF、网页、supplement、表格和扫描件解析后保留页码、section、坐标、hash、版本关系与 parser warning。
  4. 文献库可以校验 DOI/arXiv/ISBN/URL，处理 BibTeX/RIS/CSL、重复、撤稿与版本，并从正文引用跳回 source record。
  5. notes、claims、counter-evidence、methods、datasets、experiments 与 artifacts 形成带 extracted/inferred/ambiguous、confidence 和 freshness 的 evidence graph；综合矩阵并列呈现冲突证据和孤立 claim。

**Plans**: TBD
**UI hint**: yes

### Phase 13: Research 实验、论文、复现与领域 Pack

**Goal**: 研究者可以执行可复现实验并交付经过领域 verifier 检查的论文与复现包。
**Depends on**: Phase 9, Phase 12
**Requirements**: RES-07, RES-08, RES-09, RES-10, RES-11, RES-12, RES-13, RES-14, DIF-10
**Success Criteria** (what must be TRUE):

  1. 数据集和代码 intake 保留 version、license、checksum、schema、split、preprocessing、environment 与访问策略，实验固定参数、seed、输入 hash、revision 和预算。
  2. local/remote experiment 与隔离 notebook 可以 pause/resume，失败 cell 不会成为结果；统计检验、effect size、confidence interval、multiple-comparison 提示与图表均可追溯。
  3. benchmark、baseline、ablation、error analysis、robustness、negative result 和 metric selection 进入 ledger，论文 workspace 阻塞无来源数字或引用。
  4. 用户可以审查并导出 environment lock、code/data manifest、run commands、results、licenses、limitations、checksums、submission checklist、supplement 与 response-to-reviewers workspace。
  5. EDA、硬件和机器人 pack 可以注册领域 objects、tools、rules、eval 与 views；DOI、引用、数据、实验、统计、图表和外部状态 verifier 失败时输出 blocked/rework/unknown，工程签字与下单保持人工审批。

**Plans**: TBD
**UI hint**: yes

### Phase 14: Daily 对象、Connector 与审批控制

**Goal**: 知识工作者可以管理个人信息与办公连接器，并在外部写入前掌握范围和审批。
**Depends on**: Phase 9
**Requirements**: DAY-01, DAY-02, DAY-03, DAY-04, DAY-05, DAY-08, DAY-09, DAY-10
**Success Criteria** (what must be TRUE):

  1. 用户可以 create、read、update、search、link、archive 和 export 本地文件、notes、todos、reminders 与 knowledge objects，并看到来源、时间、状态和冲突。
  2. Calendar、email、message、office document 与 meeting workflow 支持时区、thread、附件、格式 warning、speaker/time provenance、preview 和服务端 ID，默认不自动发送。
  3. Connector center 展示 discover、OAuth scopes、health、last sync 与 data access，并支持 revoke、re-auth、least privilege、per-workspace enable 和 audit，凭据不进入模型上下文。
  4. workspace search 按权限裁剪本地文件、notes、mail、calendar、meeting 与获批 connector，显示来源和 freshness，断开后缓存按 policy 删除或降级。
  5. approval inbox 汇总待发送、发布、删除、付款、权限变更与 result_unknown 动作，用户可 inspect、edit、approve、deny 或 escalate，决定回写原 workflow。

**Plans**: TBD
**UI hint**: yes

### Phase 15: Daily 自动化、回执与团队 Handoff

**Goal**: 知识工作者可以安全执行跨应用和混合能力包流程，并用目标状态与回执证明结果。
**Depends on**: Phase 9, Phase 14
**Requirements**: DAY-06, DAY-07, DAY-11, DAY-12
**Success Criteria** (what must be TRUE):

  1. browser/desktop automation 在每一步显示目标身份和可视状态，支持 timeout、cancel、checkpoint、compensation 与 receipt，origin 或目标不匹配时停止。
  2. 用户可以用模板把目标拆成包含 Coding、Research、Daily task 的 DAG、board、owner、deadline、approval、report 与 bounded multi-agent 工作流。
  3. Daily verifier 会核对目标应用状态、外部 ID、回执、附件 hash、参与者与审批；无法确认时保持 result_unknown 且不自动重放。
  4. 团队成员可以共享 project、task、artifact、comment、mention 与 handoff，并检查 role visibility 和 audit；个人私有对象不会因加入团队自动共享。

**Plans**: TBD
**UI hint**: yes

### Phase 16: Terminal、Headless 与 MCP 产品闭环

**Goal**: 用户和集成方可以通过 Terminal、Headless 与 MCP 完整使用三个能力包和统一运行状态。
**Depends on**: Phase 10, Phase 11, Phase 12, Phase 13, Phase 14, Phase 15
**Requirements**: SURF-01, SURF-02, SURF-03, SURF-10
**Success Criteria** (what must be TRUE):

  1. CLI、REPL 与 TUI 支持三个 pack 的 prompt、commands、history/resume、diff、tool cards、approval、background/workflow monitor、settings/doctor 与附件，文本和 JSON/stream 输出不混淆。
  2. Headless SDK/RPC 提供版本化 API、typed events、cancel、backpressure、reconnect、approval callback、idempotency、auth 与 language-neutral examples，并为 breaking change 提供迁移路径。
  3. MCP server/client 可在 stdio、HTTP、SSE 与 WS 上互操作 tools、resources、templates、prompts、auth、capability discovery 与 errors，且 policy 与本地工具一致。
  4. 用户从每个入口都能检查 capability/readiness、provider/connector/plugin health、policy、data location、version/update、release blockers 与脱敏支持诊断并导出结果。

**Plans**: TBD
**UI hint**: yes

### Phase 17: IDE 客户端

**Goal**: 开发者可以在 VS Code 与 JetBrains 中使用同一 session、policy、workflow 和 evidence 完成 Coding 旅程。
**Depends on**: Phase 3, Phase 9, Phase 10, Phase 11, Phase 16
**Requirements**: SURF-04
**Success Criteria** (what must be TRUE):

  1. VS Code 与 JetBrains 用户可以使用项目上下文、chat、inline/patch diff、diagnostics、review、task/workflow state、approval 和 resume。
  2. IDE 重启或切换入口后显示相同的 session、IDs、events、policy 与 evidence，不建立独立持久化或权限模型。
  3. workspace 未信任、Core/client 版本不兼容或能力不支持时，IDE 明确显示原因和恢复路径并 fail safe。

**Plans**: TBD
**UI hint**: yes

### Phase 18: Desktop 与 Web/App Server

**Goal**: 用户可以通过本地 Desktop 与统一 Web/App Server 管理完整工作区、团队和运维流程。
**Depends on**: Phase 12, Phase 13, Phase 14, Phase 15, Phase 16
**Requirements**: SURF-05, SURF-06, SURF-08
**Success Criteria** (what must be TRUE):

  1. Desktop 在无账户 local mode 下提供 conversation/workspace、files/sources、artifacts、connector center、models/settings、computer-use、approval inbox、history/search 与三个 pack views。
  2. Web/App Server 提供 conversations、workflows、events、files、artifacts、team、admin 与 release operations，并支持实时 reconnect、RBAC、pagination 和 bounded history。
  3. local app server 与 hosted server 使用同一 contract；Desktop 和 Web 只提交 typed commands、消费 snapshots/events，不保存第二套任务事实。
  4. 所有入口对 loading、empty、error、offline、degraded、permission-denied 与 result-unknown 状态表现一致，并支持长内容、键盘导航、screen reader、reduced motion 与本地化。

**Plans**: TBD
**UI hint**: yes

### Phase 19: 跨入口本地连续性与平台交付

**Goal**: 无账户用户可以在支持平台和多个入口之间连续完成三个能力包的本地工作。
**Depends on**: Phase 17, Phase 18
**Requirements**: CORE-15, SURF-07, SURF-09, DIF-05, DIF-06
**Success Criteria** (what must be TRUE):

  1. 用户可以在 CLI 创建混合 workflow、IDE 查看 diff、Desktop 审批并在 Web 观察执行；所有端看到相同 IDs、events、state 与 evidence，离线冲突有明确解决。
  2. Local Personal 在无账户、无 Official Cloud 时可运行 Core 与 Coding、Research、Daily，并将 code、session、memory、workflow 与 evidence 默认留在设备。
  3. 用户可以备份、迁移、导出和彻底删除本地数据；sync 与 telemetry 均需明示 opt-in，云中断不影响本地工作。
  4. Linux/macOS 原生与 Windows/WSL 的 path、terminal、sandbox、Keychain、browser/native-host 差异经过端到端验证，不支持能力明确显示。
  5. crash、daemon restart、disconnect 或入口切换后，同一跨 pack workflow 从最后可信节点恢复，不需要导入另一套状态。

**Plans**: TBD
**UI hint**: yes

### Phase 20: Official Cloud 身份、同步与租户数据基础

**Goal**: 用户可以在不削弱 Local Personal 的前提下安全启用账户、同步和云数据生命周期。
**Depends on**: Phase 9, Phase 16, Phase 19
**Requirements**: CLOUD-01, CLOUD-02, CLOUD-05, CLOUD-07
**Success Criteria** (what must be TRUE):

  1. 用户可以选择注册或通过 OAuth 登录，管理 MFA、session/device、recovery、logout 与 account deletion；未登录仍能完整使用 Local Personal。
  2. 用户可以按范围显式启用 session、workflow、artifact 与 memory 的加密同步，并查看方向、冲突、last sync、设备、恢复状态，支持暂停、export 与删除云副本。
  3. storage、cache、queue、logs、search index、worker、connector credentials 与 support tooling 均按 tenant/workspace 隔离，并通过跨租户负面测试。
  4. 用户可以设置 retention/residency、执行 portable export 和数据删除并查看 deletion status；backup restore、tombstone、密钥轮换和同步冲突均有可验证结果。

**Plans**: TBD
**UI hint**: yes

### Phase 21: Official Cloud Worker、团队、计费与运维

**Goal**: 用户和团队可以可靠使用可计费、可观察、可恢复的 Official Cloud 服务。
**Depends on**: Phase 20
**Requirements**: CLOUD-03, CLOUD-04, CLOUD-06, CLOUD-08
**Success Criteria** (what must be TRUE):

  1. remote worker 只接收签名、限权 WorkPacket，在隔离 workspace 执行并 stream typed events；reconnect、cancel、timeout、budget、verifier 回传与 unknown-side-effect 安全可验证。
  2. 团队可以管理成员和邀请，共享 project、workflow 与 artifact，使用 comments、mentions、handoff、approval 与 activity，同时保持个人和团队数据边界。
  3. 用户和运营方可以核对 subscription、plan、quota、usage、invoice、payment failure、trial/cancel 与 entitlement ledger；计费失败不删除本地数据或锁死 export。
  4. 服务公开 status/health、SLO、rate limit、queue visibility、incident/audit、backup restore、abuse control 与脱敏 support diagnostics，云降级时本地工作继续。

**Plans**: TBD
**UI hint**: yes

### Phase 22: Enterprise 部署、身份与集中治理

**Goal**: 企业可以在联网或隔离环境安装 Kiana，并集中控制身份、策略、凭据、网络与许可。
**Depends on**: Phase 5, Phase 9, Phase 16, Phase 19
**Requirements**: ENT-01, ENT-02, ENT-03, ENT-04, ENT-06, ENT-09
**Success Criteria** (what must be TRUE):

  1. 企业 operator 可以使用 online 或 air-gapped bundle 完成 preflight、capacity check、install、upgrade、rollback、uninstall 与 data migration，失败可恢复到已验证版本。
  2. 组织可以配置 OIDC/SAML SSO、MFA policy、break-glass admin、用户停用与团队映射，认证故障按安全策略降级。
  3. user、workspace admin、security/policy admin、auditor 与 operator 的 RBAC 在 provider、model、tool、connector、plugin、data、worker 和高风险动作上生效，central deny 不能被下级覆盖。
  4. vault/HSM/KMS 支持 secret scope、rotation、revocation、audit 与 zero-secret diagnostics，worker 仅获得短期最小凭据。
  5. 受限网络支持 outbound allowlist、proxy、custom CA、offline provider、private MCP/connectors、artifact quarantine 与 egress review；离线 license 可签发、宽限、续期和审计，许可问题不破坏数据访问、export 或 recovery。

**Plans**: TBD
**UI hint**: yes

### Phase 23: Enterprise 审计、数据生命周期、DR 与支持

**Goal**: 企业 operator、auditor 和客户 owner 可以验证自托管系统的审计、恢复、运维与支持承诺。
**Depends on**: Phase 22
**Requirements**: ENT-05, ENT-07, ENT-08, ENT-10
**Success Criteria** (what must be TRUE):

  1. auditor 可以检索和导出覆盖 login、policy、approval、tool/connector、data access、admin、export 与 release 的 append-only audit，并验证 retention、legal hold 与完整性。
  2. operator 可以执行 retention/residency、workspace export/delete、backup/restore、DR、schema migration 与 integrity check 演练，恢复后 event/evidence 因果顺序保持一致。
  3. operator 可以通过 logs、metrics、traces、health/readiness、queue/worker/provider/connector dashboard、alerts 与脱敏 support bundle 定位故障并接入常见 observability stack。
  4. 目标客户可以审查 support matrix、security/privacy/licensing、漏洞修复 SLA、管理员/用户/API 手册及迁移/破坏性变更策略，并用签字 evidence 验收。

**Plans**: TBD
**UI hint**: yes

### Phase 24: 1.0 全量收敛与发布证明

**Goal**: 用户只能在全部平台、能力包、入口和商业形态具备真实发布证据后获得 Kiana 1.0。
**Depends on**: Phase 19, Phase 21, Phase 23
**Requirements**: CORE-16
**Success Criteria** (what must be TRUE):

  1. Linux/macOS 原生和 Windows/WSL 用户可以安装、升级、回滚、迁移和卸载正式 artifact，并在失败后恢复到已验证版本。
  2. 每个发布物都有可验证 signature、checksum、SBOM、license/dependency scan、provenance、release notes、doctor 与脱敏 support bundle，所有声明链接到 release proof。
  3. Coding、Research、Daily、全部入口、Official Cloud 与 Enterprise 的黄金旅程、跨入口/跨租户负面测试、故障注入和 backup restore 均以目标环境证据通过。
  4. local/external blockers 与 P0/P1 均为零，目标用户、cloud owner 与 enterprise owner 完成验收后，版本才可标记为 1.0、complete 或 production-ready。

**Plans**: TBD

## Progress

**Execution Order:**
Phases 1-9 establish shared gates. Phases 10-15 may run as parallel pack workstreams after their listed dependencies. Phases 16-19 converge product surfaces, Phases 20-23 add commercial delivery forms, and Phase 24 authorizes release language.

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. 现状基线与证据治理 | 9/12 | In Progress|  |
| 2. 可复现工具链与依赖收敛 | 尚未规划 | Not started | - |
| 3. 契约与 Schema 基线 | 尚未规划 | Not started | - |
| 4. 状态权威与投影恢复 | 尚未规划 | Not started | - |
| 5. 策略、信任、凭据与本地数据边界 | 尚未规划 | Not started | - |
| 6. RuntimeHost 与运行时抽取 | 尚未规划 | Not started | - |
| 7. Provider 能力协商与适配闭环 | 尚未规划 | Not started | - |
| 8. 可靠 Workflow、证据与副作用语义 | 尚未规划 | Not started | - |
| 9. 受限多 Agent、扩展与 Pack 契约 | 尚未规划 | Not started | - |
| 10. Coding 公开基线与仓库闭环 | 尚未规划 | Not started | - |
| 11. Coding 生态、自动化与远程闭环 | 尚未规划 | Not started | - |
| 12. Research 来源、引用与证据图谱 | 尚未规划 | Not started | - |
| 13. Research 实验、论文、复现与领域 Pack | 尚未规划 | Not started | - |
| 14. Daily 对象、Connector 与审批控制 | 尚未规划 | Not started | - |
| 15. Daily 自动化、回执与团队 Handoff | 尚未规划 | Not started | - |
| 16. Terminal、Headless 与 MCP 产品闭环 | 尚未规划 | Not started | - |
| 17. IDE 客户端 | 尚未规划 | Not started | - |
| 18. Desktop 与 Web/App Server | 尚未规划 | Not started | - |
| 19. 跨入口本地连续性与平台交付 | 尚未规划 | Not started | - |
| 20. Official Cloud 身份、同步与租户数据基础 | 尚未规划 | Not started | - |
| 21. Official Cloud Worker、团队、计费与运维 | 尚未规划 | Not started | - |
| 22. Enterprise 部署、身份与集中治理 | 尚未规划 | Not started | - |
| 23. Enterprise 审计、数据生命周期、DR 与支持 | 尚未规划 | Not started | - |
| 24. 1.0 全量收敛与发布证明 | 尚未规划 | Not started | - |
