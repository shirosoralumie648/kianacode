# Kiana Code - 产品化路线图

## 目标
从代码框架到**开箱即用的商业化产品**

## 当前状态
- ✅ Rust workspace 已能编译和测试，核心 crate 已接入同一入口链路
- ✅ 基础 CLI、SDK 执行、工具注册、session 持久化、MCP、bridge/remote 的部分关键路径已有测试覆盖
- ✅ 已补齐多轮 reference 可用性缺口：thinking streaming、bridge cancel、SDK stdio 权限请求、permission mode 透传、`can_use_tool` 上下文字段、remote 未知 control request 错误响应、remote Session WebSocket 已识别 reference 的 `initialize`/`set_model`/`set_max_thinking_tokens`/`set_permission_mode` control request、`initialize` 快速 success、listener 不支持的 mutable request 快速 error 且不吞 `can_use_tool` 权限请求、remote websocket transient close 自动重连、remote Session WebSocket 4003 可通过 `KIANA_REMOTE_REFRESH_COMMAND` 刷新 token 后重连、remote session reference-shaped user event HTTP send + CLI、Sessions API list/show/title update/create/archive + CLI、git bundle seed 生成/Files API 上传/`seed_bundle_file_id` 注入 + CLI、environment provider list/selected/default cloud create + CLI、CCR v2 env-less code session create/bridge credentials/SDK URL thin API + CLI、CCR v2 `/worker/*` register/init/state/metadata/heartbeat/events/internal-events/delivery 与带 `from_sequence_num`/`Last-Event-ID` resume 的逐事件 SSE `client_event` 读取/断线内部重连/liveness timeout/reconnect budget exhaustion、CCR v2 worker 写路径对 429/5xx 和网络瞬时错误的 bounded retry、bridge `use_code_sessions=true` 时的 CCR v2 worker transport 分流、CCR v2 收到 SSE `user` event 会上报 `running` 且连续同状态无 details 更新会按 reference 去重、CCR v2 stream-json 非终态输出转发、`stream_event` `text_delta` full-so-far 聚合与 100ms maintenance tick 独立 flush、CCR v2 client events 按 100 条/10MiB reference 边界串行批量发送并有队列背压、CCR v2 outbound permission lifecycle 会按 reference 上报 worker state：`can_use_tool` -> `requires_action`、`control_response`/`control_cancel_request` -> `running`、terminal `result` -> `idle`，internal events 按 100 条/10MiB reference 边界和 200 pending window 串行批量上报，CCR v2 bridge 会在 recv/send 路径自动写 foreground user/assistant transcript internal events，支持 foreground/subagent internal events 的 cursor 分页读取，并可通过 `remote-session code-session hydrate` 将 foreground transcript 恢复成本地 SDK session、按 agent 将 subagent transcript 落成本地 JSONL、delivery updates 按最多 64 条批量上报并在 EOF/timeout/reconnect 前 flush、bridge work ingress 重复 session 去重/token lease 刷新与毒 work 清理、Session Ingress transport 会丢弃本地已发送 UUID 的 echo 和重复 inbound user UUID 重放、bridge 满载 poll 仍处理已有 session token refresh、SDK URL bridge stream-json loop 权限回环与 `result`/非空 `assistant` 输出、mock bridge API/WS + 真实 stream-json 子 runner 的 `poll -> ack -> permission -> assistant -> stop/archive` 闭环 smoke、mock CCR v2 bridge 的 `ack -> register -> init worker -> SSE user -> delivery ack -> /worker/events assistant -> internal transcript -> reconnect` 覆盖
- ✅ 本地 agentic 工具回路已修正：工具执行层保留内部结构化结果，同时把工具专用 `map_to_api_result()` 生成的 reference-shaped `tool_result` 回灌给模型；已有 runner 层和 `kiana -p` CLI 入口层两轮 mock Anthropic `Write` smoke，覆盖用户参数解析、真实文件写入、session 持久化和第二轮请求形状；`Read` 现在向模型返回 reference 风格带行号文本，`Bash`/`PowerShell` 返回 stdout/stderr 文本，`TaskOutput` 返回 reference 风格状态/output 标签，并新增 `kiana -p --tools Read,Edit,Bash` 四轮 smoke 覆盖真实 `Read -> Edit -> Bash -> final` 链路；`WebFetch`/`WebSearch` 也已补成模型可读文本结果，并有本地 HTTP fixture 的 runner 级 `WebFetch` 两轮 smoke 覆盖；`MCP`/`ListMcpResourcesTool`/`ReadMcpResourceTool` 已补成模型可读内容或资源 JSON 文本；SDK URL bridge stream-json child 模式现在会拒绝非双向 stream-json 的假支持用法，并输出 reference 兼容的 assistant event 形态；bridge 父进程会把 terminal `result` 重排到 `assistant` 之后转发，避免远端 consumer 提前看到终态
- ✅ 第二批工具结果回灌已文本化：`TaskCreate`/`TaskGet`/`TaskList`/`TaskStop`/`TaskUpdate`、`NotebookEdit`、`discover_skills`、`LSP`、`RemoteTrigger`、`REPL`、`StructuredOutput` 都有模型面向摘要；`Tool` trait 默认映射也会把未定制工具的 object/array 结果 pretty-print 成字符串，覆盖剩余控制平面工具和 `Sleep`，避免默认路径继续把内部 JSON object 直接回灌给模型
- ✅ stream-json 输入/replay 已推进 reference parity：历史 `assistant`/`system`/`control_response` 和未知 typed 事件不会打断 `kiana -p --input-format=stream-json`，replay 模式会回放 user、assistant、control_response 事件，assistant 历史会注入新 session/无持久化执行的模型上下文，已有 session 不重复注入历史；普通 stream-json 解析遇到 reference 的 `end_session` control request 后会停止消费后续事件，SDK URL bridge stream-json child loop 也会返回 success 后立即退出，避免远端结束会话后继续处理后续 user 事件；bridge 父层和 child 层现在会识别 reference 的 `mcp_status` control request 并返回 `mcpServers` 数组，避免 MCP 状态查询直接落入 unknown error
- ✅ TUI 可用性继续补齐：非交互式 stdin/stdout 会在创建 SDK session 前返回明确错误，避免留下不可用的 “TUI ...” session；TUI REPL 已接入 `kiana-commands` registry，基础本地 slash command 不再误进模型，`/clear` 会重建 TUI session，`/doctor` 会加载真实 CLI 诊断输出，resume 列表会按当前 cwd 过滤已有 cwd 元数据的 SDK session，避免跨项目误续写；`/session reply current --record-only ...` 会刷新当前转录并显示新增 user message，而不是把 `sdk_prompt_recorded` 内部 JSON 直接丢到屏幕上；`/session compact current ...` 会在实际压缩后刷新当前转录并显示 compact summary，而不是停留在旧消息列表；`/session fork current` 会切换到 forked session，后续输入不再误写回 source session
- ✅ REPL 入口可用性补齐：无参数 `kiana` 现在会在创建 SDK session 前检查 stdin/stdout 是否是交互式终端，脚本/管道环境会得到明确错误并提示改用 `kiana -p <prompt>` 或 `kiana tui`，不再留下无意义的空 REPL session
- ✅ 本地会话管理继续收敛：顶层 `kiana session list/show/reply --record-only/rename/tag/fork/delete/export/compact` 现在复用 `kiana-commands` 的 session command 输出，显示消息数、tag、更新时间和后续操作提示，不再维护信息更少或输出不一致的平行实现；`kiana export --help` 已接上非交互帮助路径，`kiana --resume <id>` 无 prompt 时也复用同源 session show 输出，模型执行型 `kiana session reply <id> <msg>` 默认输出 assistant 文本而不是内部 SDK JSON，`kiana --continue` 现在会根据 session 的 `cwd` 元数据优先选择当前工作目录最近的本地 SDK session，避免跨项目误续写
- ✅ 发布前验收开始落地：`kiana release` 会报告 release smoke gates 和 CI workflow，`scripts/release-smoke.sh` 会串行执行格式检查、workspace 测试、release build、版本检查、doctor 检查和临时 `INSTALL_DIR` 安装后自检，GitHub Actions 复用同一个脚本；另有 `kiana remote-session code-session smoke` 作为可选真实服务连通性验收
- ✅ 顶层命令面继续补齐：`kiana auto-mode defaults|config|critique` 已接入本地命令系统，`defaults`/`config` 输出 reference-shaped `allow`、`soft_deny`、`environment` JSON，并支持 `settings.autoMode` / `settings.auto_mode` 自定义 section 替换；`auto-mode --help` 和 completion 已进入 release smoke。剩余边界是 `critique` 目前是离线结构化审计，尚未接入 reference 的 model-backed sideQuery 分析
- ✅ 插件 marketplace 已补上最小可用远程路径：`kiana plugin marketplace add <http-url>` 会缓存远端 `marketplace.json` 并使用 manifest 的 `name` 注册 marketplace；`kiana plugin marketplace add <git-url|owner/repo>` 会 clone marketplace repo 并定位 `.codex-plugin/marketplace.json` / `.claude-plugin/marketplace.json` / `marketplace.json`；`kiana plugin install name@marketplace` 现在能安装 Git marketplace 内的相对 plugin source，也能 materialize URL/git/github/git-subdir plugin source 到本地缓存再安装。新增本地 HTTP fixture、`file://` git plugin repo 和 `file://` git marketplace repo 的测试覆盖了 `add -> install -> list` 主链路。剩余边界是 npm/pip plugin source 和 GitHub marketplace 真实外网 repo live smoke
- ✅ Direct-connect reference parity 继续推进：`kiana open cc+unix:///path/to/kiana.sock -p <prompt>` 与 `kiana server --unix <path>` 已在 Unix 平台接入 Unix domain socket 的 HTTP session 创建和 WebSocket prompt loop，复用现有 direct-connect auth、session、permission prompt 和 headless 输出路径；Windows 目标会返回明确的 Unix socket platform boundary，而不是“已解析但未实现”
- 🔄 仍在逐段核对 reference parity，尤其是更完整 bridge/session ingress、remote 真实服务、CCR v2 真实服务端验证、真实 token/epoch refresh、TUI 体验和商业化打包验证
- ❌ 尚不能声明完整替代 Claude Code reference 或商业化就绪

---

## 路线图

### Phase 1: 核心 REPL（MVP）🔄
**目标**：基础交互式 CLI 可用

#### 1.1 基础设施
- [ ] 配置系统（读取 API key、settings）
- [ ] 日志系统（tracing）
- [ ] 错误处理（统一 error type）

#### 1.2 API 客户端
- [ ] Anthropic API 连接
- [ ] 流式响应处理
- [ ] Message 构建和解析

#### 1.3 简化 REPL
- [ ] 读取用户输入（readline）
- [ ] 发送到 API
- [ ] 流式输出响应
- [ ] 基础对话历史

#### 1.4 基础工具
- [ ] Read 工具（读文件）
- [ ] Write 工具（写文件）
- [ ] Bash 工具（执行命令）
- [ ] 工具调用协议

**里程碑**：`cargo run` 可以和 Claude 对话并执行基本文件操作

---

### Phase 2: 工具系统
**目标**：支持所有 50+ 工具

#### 2.1 工具注册
- [ ] 工具定义（Tool trait）
- [ ] 动态注册
- [ ] 权限系统

#### 2.2 核心工具实现
- [ ] Edit（编辑文件）
- [ ] Grep（搜索）
- [ ] Glob（文件匹配）
- [ ] Git 操作
- [ ] LSP 集成

#### 2.3 高级工具
- [ ] Agent（子 agent）
- [ ] Workflow（工作流）
- [ ] WebSearch、WebFetch
- [ ] TaskCreate/TaskList

**里程碑**：功能接近原 Claude Code

---

### Phase 3: UI 优化
**目标**：专业终端 UI

#### 3.1 Ratatui 集成
- [ ] 主循环
- [ ] 布局系统（taffy）
- [ ] 组件渲染

#### 3.2 交互组件
- [ ] 输入框（语法高亮）
- [ ] 消息显示（markdown 渲染）
- [ ] 进度指示器
- [ ] 权限对话框

#### 3.3 优化
- [ ] 快捷键
- [ ] 主题
- [ ] 动画

**里程碑**：美观的 TUI，媲美原版

---

### Phase 4: 扩展功能
**目标**：完整功能

#### 4.1 高级特性
- [ ] Skills 系统
- [ ] Plugins 系统
- [ ] MCP server 集成
- [ ] Hooks 系统

#### 4.2 多模态
- [ ] 图片支持（PDF/PNG）
- [ ] 截图（xcap）
- [ ] 电脑控制（enigo/rdev）

#### 4.3 协作
- [ ] Remote mode
- [ ] Bridge
- [ ] Multi-agent coordination

**里程碑**：功能完全对标原版

---

### Phase 5: 打包发布
**目标**：商业化就绪

#### 5.1 安装体验
- [ ] 一键安装脚本
- [ ] Homebrew formula（macOS）
- [ ] apt/yum 仓库（Linux）
- [ ] 二进制发布（GitHub Releases）

#### 5.2 文档
- [ ] 用户手册
- [ ] API 文档
- [ ] 开发者指南
- [ ] 视频教程

#### 5.3 质量
- [ ] 集成测试
- [ ] 性能测试
- [ ] CI/CD
- [ ] 错误追踪（Sentry）

#### 5.4 商业化
- [ ] License 管理
- [ ] 使用统计（可选）
- [ ] 更新机制
- [ ] 支持渠道

**里程碑**：可商业化发布

---

## 估算

### 工作量（人月）
- Phase 1: 1-2 周（核心功能）
- Phase 2: 2-3 周（工具系统）
- Phase 3: 1-2 周（UI）
- Phase 4: 2-3 周（扩展）
- Phase 5: 1 周（打包）

**总计**：7-11 周（1 人全职）

### AI 辅助加速
用 Sonnet 工作流可以加速 3-5 倍：
- 实际开发时间：2-3 周
- AI 成本：约 $100-200

---

## 下一步行动

立即开始 **Phase 1.1-1.3**：
1. 配置系统（读取 `~/.kiana/config.toml`）
2. API 客户端（Anthropic SDK）
3. 简化 REPL（stdin/stdout 交互）

预计 2-3 天完成 MVP，可以开始使用。

---

*此文档持续更新*
