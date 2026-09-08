# 三个界面：命令行一次性任务、文件夹工作台、本地网页版

> 一句话：Kiana 现在有三个用户入口——`kiana run`（一次性任务）、`kiana`（文件夹工作台）、`kiana web`（本地网页版）——它们长得不一样，但底层是同一个 DaemonHost，规则只有一套。
> 本文写的是代码现在的真实样子；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

同一个 Kiana，三个入口。想跑一件说完就完的事，用 `kiana run -- "帮我看看这个项目"`；想在一个文件夹里持续对话干活，用 `kiana`（或 `kiana --workdir 某目录`），它是终端里的对话界面；想用浏览器操作，用 `kiana web`，它在本机起一个只监听 127.0.0.1 的网页。

关键设计是：三个入口自己都不执行任何模型调用或工具。它们做的事只有三类——解析你的命令行参数、把参数组装成协议请求（RequestEnvelope）、把请求交给 DaemonHost（composition root：控制面、策略、事件账本、执行器都在这里装配起来）。真正的授权、信任检查、审批、工具执行、事件记录全部发生在 DaemonHost 下游。所以不存在"网页版规则松一点、命令行严一点"这回事：网页上要审批的操作，命令行一样要审批；未信任的项目在哪个入口都写不了盘。

代码上这也是刻意防着的：三个入口统一走 `kiana-entrypoints/src/harness_run.rs` 里的一组信封函数（`run_envelope_on_host` / `continue_envelope_on_host` / `cancel_envelope_on_host` / `receipt_envelope_on_host`），文件头注释明确写着"入口层不许重建模型循环"。另外还有个 `kiana tui` 命令，但它停在旧的 SDK 流上，不是产品路径，本文不展开。

## 现在能干什么 / 不能干什么

**能**（每条有代码出处）：

- 三个入口共用同一 DaemonHost：`kiana run`、工作台、web 的每个回合都调 `harness_run.rs` 的同一组 `*_envelope_on_host`，经进程内 transport 直达 `DaemonHost::handle`（`harness_run.rs` 的 `LocalDaemonTransport`；CURRENT_STATUS.md §2 有专门一行"CLI / Workbench / loopback Web 共用 DaemonHost"）。
- `kiana run` 支持 `--sandbox`、`--role`、`--json`，以及 `--symposium`、`--packet`、`--review`、`--close`、`--continue`、`--cancel`、`--receipt`（互斥，一次只能选一个，`cli.rs` 的 `run_main`）；不指定 sandbox 时默认只读档（`harness_run.rs` 的 `sandbox_policy_from_options`，最后兜底 `read_only_policy`）。
- `kiana`（不带子命令）进入文件夹工作台，默认 `workspace-write`（`workbench.rs` 的 `WorkbenchLaunch::default`），支持 `--workdir`、`--pick-folder`（图形目录选择器，`workbench.rs` 的 `pick_folder`）。交互模式是 ratatui 终端界面，带 `/trust`、`/sandbox`、`/receipt`、`/cancel`、`/quit` 斜杠命令（`workbench_chat.rs` 的 `interpret_line`）；加 `--json` 或环境变量 `KIANA_WORKBENCH_PLAIN=1` 退回一次性/rustyline 模式。
- `kiana web` 默认绑 `127.0.0.1:3080`，提供 run/cancel/trust/sandbox/session/receipt 六个 API 加一个页面（`web.rs` 的 `router`）；每次启动生成一个进程内随机 token 嵌进页面，后续 API 请求必须带对 token、Host 必须精确等于监听地址、带了 Origin 就必须精确匹配监听地址（`web.rs` 的 `authorize_mutation` / `url_matches_bound_addr`）。这套精确匹配有 CURRENT_STATUS.md §3 里 2026-09-07 的 P1-01 负面证据支撑：token 正确但换了个回环端口也会被 401 拒绝，且不改变项目信任状态。
- 非回环地址一律拒绝启动：`--bind 0.0.0.0:3080` 或局域网 IP 报 `bind_loopback_only`（`web.rs` 的 `parse_bind` / `ensure_loopback`）。
- 三个入口都拒绝 `danger-full-access` 这一档：无论从哪个入口传，都被显式报 `danger_full_access_rejected`（`workbench_chat.rs` 的 `normalize_sandbox`、`harness_run.rs` 的 `sandbox_policy_from_name`）。
- 工作台对未信任的文件夹会在交互模式下先问你要不要信任（`workbench.rs` 的 `ask_trust`），你答不信任时它只警告"写入会 fail-closed"，不会替你放开；一次性模式下连问都不问，直接带着未信任状态往下走，让下游策略拒绝——注释原话是"不会因入口不同意外放宽为可写执行"。

**不能**（出处是 CURRENT_STATUS.md 或代码里的明确拒绝路径）：

- 整个三入口层的状态是"代码在、局部测过，没到产品级验证"：CURRENT_STATUS.md §2 把"CLI / Workbench / loopback Web 共用 DaemonHost"标为 partial + local_behavior（局部行为验证），验证靠 cassette（录好的模型脚本）和 smoke 脚本，不是真实模型。
- 没有任何流式输出：模型回答是整段回来的，网页状态里 `"streaming": false` 是写死的（`web.rs` 的 `snapshot` 和 `health` 都直接返回这个值；CURRENT_STATUS.md 把 token streaming 标为 not_supported）。
- 会话状态在内存里，进程一退就没了：web 的会话列表、每条线程的回合记录都是 `WebApp` 里一个内存 HashMap（`web.rs`），工作台的"下次该 continue 哪个 run"游标也是本进程变量（`workbench.rs` 的 `last_run_id`）。重启之后网页上的线程全部消失，`kiana run --continue` 对已死进程的会话也续不上——CURRENT_STATUS.md 把跨进程完整 resume 标为 deferred（收据可以跨重启查，"接着干活"不行）。
- 三个界面都不接审批交互：`kiana run` 撞到需要人工审批时直接以非 Completed 状态返回、打印 "run blocked"（`cli.rs` 的 `run_main` 结尾）；工作台和网页把 AwaitingApproval 当普通"blocked"展示，网页路由里根本没有审批端点（`web.rs` 的 `router` 里没有 approval 路由）。带人工批准的自动同意开关（`command_dispatch.rs` 的 `should_auto_approve_local_write`）只在本地命令分发路径上生效，不在三个对话主路径上。
- 网页 token 不是持久的身份凭证：它随进程生成、不落盘，模块注释明说"不应被视为跨进程认证机制"（`web.rs` 的 `run_web` 文档注释）；CURRENT_STATUS.md 的 P1-01 limitations 也写明没有持久 authenticated principal、没有会话恢复、没有 OS 级本地用户边界。
- 网页健康检查 `/api/health` 故意不鉴权（`web.rs` 的 `health` 没有 authorize 调用），且对不用浏览器的直连客户端 Origin 是可选的——这是记录在案的现状，不是漏洞修好后的结论。

## 代码怎么跑（走读）

**第一步：你敲命令，先分流。** 进程入口在 `kiana-entrypoints/src/cli.rs` 的 `main_with_args`，它按固定顺序试：第一个词是 `run` 走一次性任务，是 `web` 走网页版，看起来像工作台调用（`workbench.rs` 的 `is_workbench_invocation`，包括 `workbench`/`wb`/`gui`/`--workdir`，还有"长得像目录路径"的启发式判断）走工作台。一个词都不带、只有终端时，`cli_main`（`cli.rs:13999`）直接进工作台的当前目录交互模式。

**第二步：`kiana run` 把选项变成请求。** `run_main`（`cli.rs:331`）手工解析参数，把 `--sandbox`/`--role` 等塞进一个 options 表，然后按你选的子命令调对应的信封函数——普通提示词走 `harness_run.rs` 的 `run_envelope`。这个函数先调 `new_local_host_with_options`：从 options 里挑出 provider/api_key/base_url/model 四项装进 `LocalModelConfig`（本次运行的模型偏好，怎么变成真模型是模型执行篇的事），造出一个 DaemonHost。再调 `client_on_host`（`harness_run.rs:203`）：从 options 或当前目录定出项目根，读这个项目的信任文件，组装 RequestMetadata——操作者固定写成本地主体 `local-user`（客户端自称谁不算数）、信任状态以磁盘上读到的为准、sandbox 名字翻译成权限档位（只读→Safe 档，workspace-write→Balanced 档）、指定了角色就查 RoleSpec 花名册并赋角色。最后通过进程内 transport 把请求送进 `DaemonHost::handle`，从这一步起就是审批中枢那套流程了（详见审批和能力脊柱两篇）。

**第三步：`kiana` 工作台多做的几件事。** `run_workbench`（`workbench.rs:203`）先解析并切换到工作目录（`resolve_workdir` 会规范化路径、拒绝不存在的目录），然后检查信任：交互终端里弹 y/N 提问你信不信任这个文件夹。接着造 `DaemonHost::local()` 和一个随机 session ID。如果 stdin/stdout 都是终端且没要 JSON 模式，进入 `workbench_chat.rs` 的 ratatui 界面循环：你的输入经 `interpret_line` 判定是斜杠命令还是提示词，提示词经 `submit_turn` 丢进一个 tokio 任务去调 `run_envelope_on_host`（第一次）或 `continue_envelope_on_host`（之后，带上记住的 run_id），daemon 的响应经 channel 回到主循环，`apply_response` 把助手文本和声明的文件变更投影到屏幕。注意首行注释里的自我声明：屏幕上的 running/idle 和"已完成"都只是界面观察，不是持久事实，审计要看正式 receipt（`/receipt` 命令随时可查）。不是终端（比如脚本里）就退化成一次性：跑完一个 prompt，状态不是 Completed 就报错退出（`finish_status`）。

**第四步：`kiana web` 起服务。** `run_web`（`web.rs:295`）解析参数后先过 `parse_bind`——这一步就把非回环地址挡死了。绑定成功后造 `DaemonHost::local()` 和 `WebApp`：里面装着 DaemonHost、工作目录、当前 sandbox/role（各一把锁）、会话 HashMap，还有一个 `uuid::Uuid::new_v4()` 生成的进程内 token。启动 axum 服务，打印 URL，除非 `--no-open` 否则尝试用系统命令打开浏览器。页面请求 `GET /` 时把 token 替换进 HTML 交给浏览器；之后的每个 API 请求都要过 `authorize_mutation`（`web.rs:622`）：① 请求头 `x-kiana-web-token` 必须和进程 token 一字不差；② Host 头解析出来的 IP 和端口必须精确等于实际监听地址（不是"是回环就行"，是"就是这个端口"）；③ 浏览器会带的 Origin 头如果出现，也必须精确匹配监听地址。三关全过才轮到业务：`run_turn` 校验提示词（非空、不超过 64KB）、定位会话（最多 128 个）、防止同一会话并发跑两个回合（`session_busy`），然后调的仍是第二步那组信封函数——web 没有自己的执行路径。响应回来后 `store_turn` 把它裁剪成线程视图（`web_thread.rs` 的 `items_from_turn`：用户消息、用了哪些工具、改了哪些文件、助手文本、错误），每条线程最多留 256 个回合、1MB 文本，超了就从最旧开始丢——这些投影只管显示，EventLog 和协议回执才是事实来源。

**失败分支一：有人从别的端口/源打进来。** 比如你把页面开在 3080，本机另一个程序拿着偷到的 token 去请求 3081 上另一个 Kiana 实例：token 可能对得上（如果两边都猜中），但 Host/Origin 对不上 3081 实例的监听地址，`authorize_mutation` 返回 401 `web_auth_required`。这个场景有专门的集成测试和 CURRENT_STATUS.md 的负面证据记录（2026-09-07 的 P1-01 块），测试名就叫 `web_rejects_wrong_origin_and_host_without_mutating_trust`。

**失败分支二：项目没被信任。** 三个入口对未信任项目的处理不同但结局一致：`kiana run --sandbox workspace-write` 会一路把"未信任"写进 RequestMetadata，下游策略引擎第一条就拒绝；工作台一次性模式同样不拦不劝，让同一个拒绝码原样浮上来；网页上你可以点"信任此文件夹"按钮（走 `/api/trust`，同样要过 token/Host/Origin 三关）把信任写进项目，写完才能干活。哪个入口都不会因为"你是从界面点的"就跳过下游检查。

## 关键概念速查

| 概念 | 一句话解释 | 代码在哪 |
|---|---|---|
| DaemonHost | 组装根（composition root）：控制面、策略、事件账本、执行器在这里拼装，三个入口共用这一个 | `kiana-daemon/src/lib.rs`；由 `harness_run.rs` 的 `new_local_host_with_options` / `DaemonHost::local` 创建 |
| `harness_run.rs` 信封函数 | `run/continue/cancel/receipt_envelope(_on_host)`：所有入口统一的"发请求"方式，禁止入口层自建执行循环 | `kiana-entrypoints/src/harness_run.rs` |
| RequestMetadata | 请求的"来访者登记表"：本地主体 `local-user`、项目路径、信任状态、权限档位、角色 | `harness_run.rs` 的 `client_on_host` |
| `sandbox_policy_from_options` | 把 sandbox 名字翻译成档位：read-only→Safe，workspace-write→Balanced，danger-full-access 直接拒绝 | `harness_run.rs:456` |
| WorkbenchLaunch | 工作台的启动配置：工作目录、sandbox（默认 workspace-write）、角色、是否一次性 | `workbench.rs:40` |
| 斜杠命令 | 工作台终端里的 `/trust` `/sandbox` `/receipt` `/cancel` `/quit`，只改 UI 状态或发对应 daemon 请求 | `workbench_chat.rs` 的 `interpret_line` |
| web token | 每次进程启动随机生成的 UUID，嵌进页面，API 请求必须原样带回；进程内有效、不落盘 | `web.rs` 的 `WebApp::new`、`authorize_mutation` |
| Host/Origin 精确匹配 | 请求头里的 Host 和 Origin 必须精确等于监听的 IP:端口（scheme 必须 http、路径必须 `/`），不是"回环就算" | `web.rs` 的 `url_matches_bound_addr` |
| ThreadView / TurnView / ItemView | 回执到网页的展示投影：线程=会话、回合=一次 run/continue、条目=用户消息/工具/文件/文本 | `web_thread.rs` |
| `bind_loopback_only` | 非 127.0.0.1/::1 的监听地址一律拒绝启动的错误码 | `web.rs` 的 `ensure_loopback` |
| `local_behavior` | CURRENT_STATUS.md 的证明等级："代码在、局部测过"，不等于产品级验证 | CURRENT_STATUS.md §1 |

## 设计视角：现在最明显的短板

以下都是基于代码现状的观察，不是改进建议。

1. **审批没有接到任何界面上。** 审批链路本身是通的（审批单落盘、指纹校验都在），但三个界面都不会弹审批：`kiana run` 返回 blocked，工作台和网页把 AwaitingApproval 当错误展示，网页连审批端点都没有。也就是说在对话主路径上，需要人工批准的操作目前只能"被拒"，人没有机会当场说"允许"。
2. **网页的全部会话状态活在进程里。** `WebApp` 的 sessions HashMap、每条线程的 run_id 游标、信任按钮写下去之前的状态，重启全没；浏览器刷新一次靠重新拉 `/api/state` 还能救，进程重启就是另一回事。CURRENT_STATUS.md 也把"Web session ownership、异步状态和多标签页隔离仍有缺口"记在同一行里。
3. **一个回合就是一次完整等待。** 没有 streaming，`run_turn` 是同步等整个模型回合跑完才返回；回合长的时候浏览器就一直是 running，中间没有任何进展可看。
4. **token 防的是"别的网页"，不防"本机程序"。** 页面 token 是防 DNS rebinding 和跨站请求的，但任何本机进程只要读到页面内容就拿到 token，且 OS 层面没有用户边界——CURRENT_STATUS.md 的 P1-01 limitations 原文承认没有 durable authenticated principal 和 OS-level local-user boundary。健康端点不鉴权、非浏览器客户端 Origin 可选，也是有意的现状。
5. **三个入口的参数解析是三份独立代码。** `--sandbox`/`--role` 的解析和档位映射在 `run_main`、`parse_workbench_args`、`parse_web_args` 里各写一遍，靠 `normalize_sandbox` 和 `sandbox_policy_from_*` 这两个共享函数兜住一致性；新加一个入口或一个选项，需要记得三处同步。
6. **交互工作台的一次性模式和交互模式体验分叉。** 同一个 `kiana` 命令，接不接终端决定走 ratatui 界面还是一次性输出，两条路对"未信任项目"的提示策略不同（一个先问，一个不问）；行为都对，但用户感知不到这是同一个东西在背后。

## 相关文档

- `docs/company-os-overview.md`：§2.2"全部入口一览"（三个入口的命令表和各自产物）、§2.3"现在明确不能声称的"（无 streaming、不能跨重启续跑）、§3 对照表里"总装配车间=DaemonHost，所有入口共用这一个"。
- `CURRENT_STATUS.md`：§2 能力表"CLI / Workbench / loopback Web 共用 DaemonHost | partial | local_behavior"和"live provider / token streaming | not_supported"、"跨进程完整 resume | deferred"；§3 的"P1-01 Web exact-listener Host/Origin denial evidence (2026-09-07)"；§4 的 P1-01（authenticated principal 与 session ownership）。
- `docs/features/` 兄弟文档：01（模型执行——`LocalModelConfig` 里那四项怎么变成真实 provider 或 cassette）、04（信任与 sandbox——`project_trusted` 和档位映射下游发生什么）、05（审批——三个界面目前都不接的那条链路）、06（EventLog 与 Receipt——`/receipt` 和网页投影背后的权威数据）、02（工具调用脊柱——所有入口最终汇入的执行主路径）。
