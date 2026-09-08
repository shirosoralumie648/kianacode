# 五个模型可见工具（shell / apply_patch / mcp / memory.search / memory.write）

> 一句话：模型在 Kiana 里能用的"手"只有五只——跑命令、打补丁、调外部 MCP 工具、查记忆、写记忆；这份清单是写死的，一只都不多，这是有意设计。
> 本文写的是代码现在的真实样子；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

模型不是直接碰你的电脑。它每说一次"我要调某个工具"，Kiana 就把这句话翻译成一份正式申请（CapabilityRequest），走 02 篇讲的"审批—派发—执行—留痕"流水线，最后由真正的 handler（执行器）干活。本文讲的就是这五个 handler 各自长什么样：模型怎么调、参数长什么样、每一层检查什么、哪里会硬拒。

工具面锁死为五个，写在 `kiana-runner/src/tools.rs` 的 `tool_schemas`，连测试都断言名单恰好是这五个（`tools.rs` 的测试 `tool_schema_exposes_exactly_the_five_fixed_names`）。为什么这么死？因为**每新增一个工具就多一个授权面和攻击面**：一个工具要同时在角色目录（这个角色能不能用）、策略映射（`kiana-policy` 的 `harness_tool_name`）、风险分级（只读还是本地写还是对外副作用）、broker 注册表（哪个 handler 接）、拒绝测试（近似名和注入名不能漏过）五处登记，少登记任何一处都是漏洞。仓库里 `kiana-tools` 其实有 50 多个现成工具，但 AGENTS.md 的 FZ-TOOLS 条目明文冻结："不许把 `kiana-tools` 的 50+ 工具接到 harness"。五个工具的 schema 注释里也写死了规矩：新增工具必须同时更新角色目录、策略映射和拒绝测试，不能只在 schema 列表里追加一项。

## 现在能干什么 / 不能干什么

**能**（每条都有代码或 CURRENT_STATUS 出处）：

- 模型只看得见五个工具：shell、apply_patch、mcp、memory.search、memory.write；少量兼容别名（`bash`/`exec`/`command_execution`/`file_change`/`mcp.call`）会归并到规范名，其余一律拒绝（`kiana-runner/src/tools.rs:capability_for_tool`）。
- shell 在 Linux 上用 bubblewrap（bwrap，一种沙箱程序）隔离执行：新会话、隔离全部命名空间、根目录整体只读挂载、项目根按档位重新挂载、清空全部能力位和环境变量（`kiana-daemon/src/harness_sandbox.rs:bwrap_plan`）。默认 30 秒超时，模型可以传 `timeout_ms` 但封顶 60 秒；stdout/stderr 各自 1MB 封顶（`harness_capabilities.rs` 的 `COMMAND_TIMEOUT` / `command_timeout` / `EXEC_OUTPUT_MAX_BYTES`）。
- shell 超时会先 SIGTERM 再 SIGKILL 杀掉整个进程组（含子进程），确认杀干净才返回；确认不了就报 `shell_result_unknown:process_group_not_stopped`，不装作没事（`harness_capabilities.rs:terminate_process_group`）。
- shell 传进沙箱的环境变量走白名单：只继承 11 个核心变量，名字里含 KEY/SECRET/TOKEN 的一律剔除，`TMPDIR` 强制指向沙箱内的 /tmp（`harness_sandbox.rs:sandbox_env`）。
- apply_patch 只在 workspace-write 档可用，只读档直接报 `apply_patch_requires_workspace_write`（`harness_capabilities.rs:ApplyPatchHandler::execute`）。
- apply_patch 整份补丁先在内存里预检（解析、路径圈禁、逐块定位），全部通过才落盘；落盘中途失败会把已完成的操作回滚，回滚也失败则报 `apply_patch_rollback_failed`（`kiana-daemon/src/apply_patch.rs:apply_codex_patch`、`rollback_preconditions`）。
- mcp 只走 stdio（本地子进程）方式；参数按对方服务器通告的 schema 逐条校验后才发出去（`kiana-daemon/src/harness_mcp.rs:McpCallHandler::execute`）。CURRENT_STATUS.md 的 P1-06 证据块记录了这条链的局部验证。
- memory 按六层 collection（company / department / role / project / user / instance-scratch）分文件存储，读和写都做角色 ACL（`kiana-daemon/src/harness_memory.rs`、`kiana-policy/src/lib.rs:memory_decision`）。
- shell 的输出在写进事件账本、回执和下一轮模型上下文之前会先脱敏；CURRENT_STATUS.md 的 P1-05 证据块（2026-09-07）记录了四类哨兵密钥（argv、stdout、stderr、环境变量各一路）端到端不出现在任何持久面的回归。

**不能**（出处是 CURRENT_STATUS.md 或代码里的明确拒绝路径）：

- **整条链的现状是"代码在、局部测过，没到产品级验证"**：CURRENT_STATUS.md §2 把 `shell / apply_patch broker 主路径` 标为 partial + local_behavior，边界一栏原话是"仍需加强 TOCTOU、原子 patch、进程树取消和输出 redaction"。
- 模型看不到第六个工具，`kiana-tools` 的 50+ 工具也不在面内（FZ-TOOLS，AGENTS.md）。
- 不存在 `danger-full-access` 之类的无沙箱档位：未知档位报 `sandbox_unsupported`（`harness_sandbox.rs:bwrap_plan` 的 match 分支）；机器上没装 bwrap 时报 `sandbox_unavailable:bwrap`，**不会退化成不沙箱直接跑**。
- read-only 档下 shell 物理上写不进工作区：项目根以只读方式重新挂载，测试 `shell_exec_read_only_cannot_write_workspace` 验证了这一点。
- apply_patch 拒绝符号链接路径（含路径中任何一层是 symlink）、拒绝 hardlink 目标（`apply_patch.rs:reject_symlink_components` / `reject_hardlink`）；memory 文件读写同样用 `O_NOFOLLOW` 拒绝 symlink（`harness_memory.rs:read_records` / `append_record`）。
- memory.write 不许跨层提升：`promote_to` 只能等于原 collection，否则报 `role_memory_promote_denied`（`kiana-policy/src/lib.rs:memory_decision`）；聊天内容也永远不会自动进记忆库（`harness_memory.rs` 模块注释）。
- MCP 的 HTTP/SSE/WS 传输报 `mcp_transport_unsupported`；未知服务器、未知工具、重名工具、参数超限（工具名 256 字节、参数 64KB、结果 256KB）全部 fail closed（`harness_mcp.rs` 各分支）。
- 单次 shell 超时上限 60 秒写死在代码里（`command_timeout` 的 `min(60_000)`），模型想跑长任务只能自己分步。

## 代码怎么跑（走读）

公共链路在 02 篇已经完整走过：模型声明调工具 → `capability_for_tool` 翻译成 CapabilityRequest（每个工具在这里就被定了风险等级）→ ControlPlane 查角色工具权限和 collection ACL → 政策层按风险决定放行还是审批 → broker 按（能力类型，操作名）精确派发 → handler 执行。本篇从 handler 往下讲。

### shell：把命令关进沙箱再放手

模型传三个参数：`command`（字符串或字符串数组，schema 见 `tools.rs:tool_schemas`）、可选 `workdir`、可选 `timeout_ms`。翻译时风险按档位初判：workspace-write 记本地写，其余记只读——`shell_risk` 的注释自己承认这只是粗分，不能由此推断命令内部一定没有外部副作用（比如它 curl 一下外网）。

handler（`harness_capabilities.rs:ShellExecHandler::execute`）拿到申请后不信任参数，逐个重验：

1. **命令形状**：字符串形式会被包成 `/bin/sh -c <整串>`；数组形式按 argv 原样使用（`command_argv`）。也就是说字符串形式走的是 shell 解释，数组形式不走——这是模型可选的两种表达，不是两级安全。
2. **工作目录**：`workdir` 先 canonicalize（消掉符号链接和相对路径），必须落在项目根之内，含 `..` 直接报 `harness_workdir_not_relative`（`confined_workdir`）。
3. **构造沙箱计划**：`bwrap_plan`（`harness_sandbox.rs`）生成一串 bubblewrap 参数：`--new-session`（新会话，脱离终端控制）、`--die-with-parent`（父进程死则跟着死）、`--unshare-all`（隔离全部内核命名空间，网络也在内）、根目录整体 `--ro-bind` 成只读、`/tmp` 挂成新的 tmpfs，然后按档位重新发布项目根——read-only 再挂一次只读，workspace-write 挂成可写。顺序在这里很重要：read-only 档的项目根重挂必须发生在 `/tmp` tmpfs 之后，否则工作区在 /tmp 下时会凭空消失（代码注释原话解释了这一点）。最后 `--cap-drop ALL` 清掉全部能力位，`--clearenv` 清空环境，再把白名单环境变量一个个 `--setenv` 塞回去。bwrap 可执行文件从 `KIANA_BWRAP` 环境变量或 PATH 里找，找不到就报错——没有"找不到就裸跑"的分支。
4. **执行**：外层 bwrap 进程自身也 `env_clear`（注释引 Codex 的理由：不让宿主密钥留在 bwrap 的 /proc 环境，即使沙箱内看不到，宿主侧进程内存里也不该有）。stdout/stderr 各由一个异步任务边读边截断在 1MB，超限加 `...truncated...` 标记。
5. **超时与进程组**：命令先放进独立进程组。到时后先 SIGTERM 整组，短暂宽限后仍活着就 SIGKILL，然后按 Codex 的语义把超时当作一种执行结果返回：exit code 124、`timed_out: true`——不是崩溃。如果进程组没能确认停止，报 `shell_result_unknown`，宁可承认"结果不明"。

### apply_patch：先验货，再签收

模型传一份 Codex 格式的补丁文本（`*** Begin Patch` 开头，里面是 Add File / Delete File / Update File / Move to 各段）。schema 里还有个可选 `path` 参数——它不参与定位文件，只被政策层拿去做路径允许清单检查（`kiana-policy/src/lib.rs:request_paths` 会同时提取 `path` 字段和补丁头里的所有路径）；真正改哪个文件，完全由补丁文本自己说。

整个执行（`apply_patch.rs:apply_codex_patch`）分两个大阶段，中间隔着一次"验货"：

**预检阶段（不碰磁盘上的任何内容）**：

1. `parse_patch` 解析成 hunk 列表；空补丁报 `apply_patch_empty`。
2. `plan_hunks` 在一张内存 overlay（草稿层）上逐 hunk 规划：改完这个文件后它在草稿层里长什么样、下一个 hunk 基于哪个版本匹配。Update 块要求旧内容在文件里**唯一**匹配——找不到报 `apply_patch_hunk_mismatch`，找到多处报 `apply_patch_hunk_ambiguous`，宁可不改也不猜。
3. 每个涉及的路径做圈禁检查：必须是相对路径、不能有 `..`、路径中任何一层都不许是 symlink、目标是已有文件时不许是 hardlink（`reject_symlink_components` / `reject_hardlink`——这两手专门防"用链接把工作区内的路径偷换成工作区外的文件"）、最后 canonicalize 后必须仍在项目根内。
4. 抓取前置快照（`capture_preconditions`）：对每个涉及路径**连同它到项目根之间的每一层父目录**，记一份指纹（是否文件/目录/symlink、长度、修改时间、device + inode 号）和文件完整内容。

**提交阶段（验货通过才签收）**：

5. 先抢项目级文件锁（`ProjectPatchLock`，flock 在 `~/.kiana/locks/` 下按项目路径哈希命中的锁文件上），并发补丁在此排队。
6. 把快照里所有"存在"的父目录用 `O_DIRECTORY | O_NOFOLLOW` 打开成文件描述符（`open_commit_directories`），然后**再验一遍快照**（`verify_preconditions`）：预检之后到现在，任何路径的指纹变了就报 `apply_patch_path_changed` 整体放弃。这就是在收窄 TOCTOU 窗口："我看的时候它长这样，落笔前它必须还是这样"。
7. 真正写盘时全部走这些已打开的目录描述符（`openat` / `unlinkat` / `renameat`），而不是重新按路径找目录。这是"描述符锚定"：即使验证之后有人把目录整个换掉（测试里就模拟了 rename 目录再塞一个同名新目录），写入也落在**预检时的那个目录**上，新目录收不到。
8. 改已有文件用原子替换：在**同一目录**下用 `O_CREAT | O_EXCL | O_NOFOLLOW`（独占创建、不跟随链接）造一个 `.名字.kiana-patch-进程号-时间戳-序号` 的临时兄弟文件（模式 0600），写完恢复原文件权限，再 `renameat` 一步换名——rename 在同一文件系统内是原子的，读者要么看到旧文件要么看到新文件。临时名撞了就换下一个重试，16 次都占满就报 `apply_patch_temp_unavailable` 放弃，绝不复用一个已存在的文件。
9. 任何一步失败，按快照**从深到浅**回滚已完成操作（`rollback_preconditions`）：删掉多出来的文件、把改过的文件原子换回原内容并恢复只读位。回滚本身失败则报 `apply_patch_rollback_failed`，把两个错误都亮出来。

### mcp：按对方的名片逐项核对

模型传 `server`（可选）、`tool`（必填，`tool_name` 是兼容写法）、`arguments` 对象。这个工具在翻译时就被定为 ExternalSideEffect（对外副作用）——政策层强制它必须人工审批，且任何人都不能把它降级成低风险绕过（02 篇讲过的 `mcp_risk_downgrade` 拒绝）。

handler（`harness_mcp.rs:McpCallHandler::execute`）每次调用都从零来一遍：

1. **找服务器清单**：从环境变量 `KIANA_MCP_SERVERS_JSON` 读（上限 64KB），不是模型传的——模型只能点名清单里已有的服务器，报 `mcp_server_unknown` 就是没有。不点名且清单里恰好只有一台时才自动选定。
2. **只认 stdio**：配置的 transport 不是 Stdio 直接报 `mcp_transport_unsupported`。HTTP MCP 在 CURRENT_STATUS.md 和 AGENTS.md 里都是明文不支持项。
3. **问对方要工具目录**：连接后先 `list_tools`，在通告清单里找请求的工具名——没有报 `mcp_tool_unknown`，重名报 `mcp_tool_ambiguous`。**校验的基准是对方此刻通告的 schema，不是模型转述的**。
4. **schema 白名单校验**：只接受一个小而可互操作的 JSON-Schema 子集（type/properties/required/enum/长度和数值边界等，深度上限 32）；出现 `oneOf` 这类不支持的关键字时报 `mcp_tool_schema_unsupported`——注释说得很清楚：不支持的约束**拒绝**，而不是悄悄忽略。因为一个校验器看不懂的约束等于没有约束，放过去就是假装验过。
5. **逐参数校验**：required 缺了报 `mcp_argument_required`，类型不对报 `mcp_argument_type_invalid`，`additionalProperties: false` 时多传字段报 `mcp_argument_unknown`，enum/const/min/max 逐条核对。
6. **调完验结果**：返回体超过 256KB 拒收；形状不对（content 不是带 type 的数组、也没有 structuredContent）报 `mcp_result_invalid`；对方标了 `isError: true` 的话，结果照样带回来，但 CapabilityResult 的 success 是 false——工具执行失败不等于申请失败，模型会看到失败内容自己决定下一步。

### memory.search / memory.write：按角色给钥匙的 JSONL 记忆库

两个工具共用一套存储：六层 collection（company / department / role / project / user / instance-scratch），每层落成对应路径下的 JSONL 文件——company/user 等全局层在 `KIANA_HOME/memory/` 下，department/role/project 等项目层在项目的 `.kiana/memory/` 下，instance-scratch 按 session 隔离且 session 名禁止含 `/`、`\`、`..`（`harness_memory.rs:collection_path`）。读写都用 `O_NOFOLLOW`，文件被换成 symlink 就直接报错。

模型传的 `role_id` 说了不算：ControlPlane 派发前用 `stamp_request_identity`（`kiana-core/src/lib.rs`）把核对过的角色、部门、会话、项目根盖进参数——handler 拿到的是盖章版本。

- **search**：`query` 必填，匹配是朴素的子串包含（把所有空格分开的词都要求出现在文本里，不分大小写，`text_matches`）——不是向量检索。指定 `collection` 时，政策层先查角色的知识权限（`role.allows_knowledge`，不过关报 `role_knowledge_denied`，比如 Builder 查 `user-private` 或 `project:events` 都会被拒）；**不指定**时自动搜角色被授权的全部 collection（`granted_collections`）。`limit` 默认 10。每条命中带 `layer` / `collection` / `source` / `verified` 字段——`verified` 的判定仅仅是 source 字段非空，schema 描述里也提醒模型：没有 source 的命中不是经过验证的结论。
- **write**：`collection`、`text`、`source` 三者必填；政策层查 `role.allows_memory_write`（测试里 Builder 写 `project` 层被拒，`role_memory_write_denied`）；`promote_to` 若填必须等于原 collection——这条封死了"借一次写入把内容从低层跨到高层"的路。记录由 handler 生成 id（`mem-<毫秒时间戳>`）并原样追加一行 JSON，不做任何自动归纳或改写。

## 关键概念速查

| 概念 | 白话解释 | 代码在哪 |
|---|---|---|
| 工具面（五个固定工具） | 模型唯一能调用的工具名单，写死且有测试钉住 | `kiana-runner/src/tools.rs:tool_schemas` |
| FZ-TOOLS | 冻结条款：不许把 `kiana-tools` 的 50+ 工具接进 harness | `AGENTS.md` 的 FZ-TOOLS 条目 |
| bubblewrap（bwrap） | Linux 命令行沙箱工具，用内核命名空间和绑定挂载隔离进程 | `kiana-daemon/src/harness_sandbox.rs:bwrap_plan` |
| `--ro-bind` / `--bind` | bwrap 的只读/可读可写目录挂载；read-only 档项目根被只读重挂 | `harness_sandbox.rs` 的 match 分支 |
| 环境变量白名单 | 沙箱内只继承 11 个核心变量，含 KEY/SECRET/TOKEN 名字的一律剔除 | `harness_sandbox.rs:UNIX_CORE_ENV_VARS`、`sandbox_env` |
| 进程组终止 | 超时先 SIGTERM 后 SIGKILL 整个进程组，确认停止才算数 | `harness_capabilities.rs:terminate_process_group` |
| 前置快照（precondition） | 补丁涉及路径及每层父目录在预检时的指纹 + 内容备份 | `apply_patch.rs:capture_preconditions` |
| 描述符锚定 | 提交时通过预检时打开的目录 fd 写盘，目录被偷换也写不进新目录 | `apply_patch.rs:open_commit_directories`、`commit_op` |
| 临时兄弟文件 | 同目录独占创建、写完 rename 的中间文件，保证替换原子 | `apply_patch.rs:atomic_replace_bytes_at` |
| advertised schema | MCP 服务器自己通告的工具参数 schema，参数校验以此为准 | `harness_mcp.rs:select_advertised_tool`、`validate_tool_schema` |
| 六层 memory collection | company/department/role/project/user/instance-scratch，各落各的 JSONL 文件 | `kiana-domain` 的 `MEMORY_LAYERS`；`harness_memory.rs:collection_path` |
| `promote_to` 限制 | 写记忆时的提升目标只能等于原 collection，禁止跨层搬内容 | `kiana-policy/src/lib.rs:memory_decision` |
| `stamp_request_identity` | ControlPlane 把核对过的角色/会话/项目根盖进申请参数，模型自报的无效 | `kiana-core/src/lib.rs:stamp_request_identity` |

## 设计视角：现在最明显的短板

以下都是基于代码现状的观察，不是改进建议。

1. **shell 的命令内容本身没有白名单。** 沙箱挡住了文件系统写入和网络，但命令字符串（`/bin/sh -c` 形式）里写什么都照跑；风险等级只按档位粗分，`shell_risk` 的注释自己承认不能推断命令没有外部副作用。挡住的是"碰哪里"，不是"干什么"。
2. **预检与提交之间的窗口只是收窄，不是关死。** verify_preconditions 在提交前重验了快照，CURRENT_STATUS.md §2 仍把 effect-time TOCTOU 列在 P1-04 的未完成边界里；描述符锚定覆盖了"目录被换"，但不覆盖所有可能的并发改动路径。
3. **memory.search 是朴素子串匹配。** `text_matches` 要求每个空格分词都出现在文本里，既没有相关性排序也没有向量/索引支持，collection 一大或者 query 措辞稍偏就查不到——能力上限是"关键词全文过滤"。
4. **MCP 的信任起点是环境变量。** 服务器清单来自 `KIANA_MCP_SERVERS_JSON`，schema 和工具目录每次调用时向对方现取；P1-06 证据块的 limitations 原话承认：配置的 stdio 发现发生在审批之后，但"不是 durable 的来源记录"，服务器可执行文件与配置的持久化来源证明仍是空白。
5. **超时上限、输出上限都是常量。** 60 秒超时、1MB 输出、256KB MCP 结果、64KB MCP 参数全部写死在代码里（`harness_capabilities.rs`、`harness_mcp.rs` 的常量），长任务和大数据量场景要么被截断要么被拒，且模型侧没有被告知这些上限的途径（schema 描述里没写）。

## 相关文档

- `docs/company-os-overview.md`：§5 术语表里的 sandbox、CapabilityRequest、MCP 词条；"一次 run 的十步流程"第 ⑤–⑩ 步是本文公共链路的白话版。
- `CURRENT_STATUS.md`：§2 能力表的 `shell / apply_patch broker 主路径` 和 `基础 trust / sandbox / role / path / memory policy` 两行（partial + local_behavior）；§4 的 P1-04（apply_patch 原子性与 TOCTOU）、P1-05（输出脱敏）、P1-06（MCP 风险边界）。
- `docs/features/` 兄弟文档：01（工具 schema 是从模型循环的第 7 步发出去的）、02（CapabilityRequest 的申请—审批—派发全链，本文是它五个终点的放大镜）、04（trust 和 sandbox 档位从哪来、path lock 怎么防撞车）、05（mcp 必经的人工审批长什么样）、06（工具结果脱敏后落在哪本账上）、10（模型搜代码的另一半路：跑 rg，不走 memory）；全部篇目见 `docs/features/README.md` 索引。
