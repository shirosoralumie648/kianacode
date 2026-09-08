# 信任、沙箱与路径安全（谁能碰哪些文件）

> 一句话：这一层决定"AI 在哪个目录里、能不能写盘、能写哪些文件、两个任务会不会互相踩脚"。
> 本文写的是代码现在的真实样子，不是产品愿景；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

这一层有三道机制：

- **信任（trust）**：对项目目录的一次显式声明。你先跑 `kiana trust .`，Kiana 才允许在这个项目里干活；没信任的项目，AI 连只读操作都会被拒绝。
- **沙箱（sandbox）**：AI 执行命令不是直接在你的系统上跑，而是在 Linux 的 bubblewrap 隔离环境里跑：默认整个磁盘只读挂载，明确要求 workspace-write 时才把项目根挂成可写，并且默认断网。
- **路径锁（path lock）**：两个并行任务同时想动同一路径时，只有先到者能拿到锁，后来的任务直接被拒。

## 现在能干什么 / 不能干什么

**能（代码里都有出处）：**

- `kiana trust .` 会把"这个项目我信了"写成一条记录，存到你**家目录**（`~/.kiana/trust/projects/` 下，文件名是项目路径的指纹），不是存在项目里——这是故意的，防止项目自己给自己发信任凭证（`kiana-types/src/trust.rs:write_project_trust`）。
- 项目没信任时，**连只读操作都直接拒绝**，原因码是 `project_untrusted`（`kiana-policy/src/lib.rs:DefaultPolicyEngine::evaluate` 的第一道判断）。
- 只读沙箱：整个磁盘以只读方式挂进隔离环境，项目根也重新挂成只读（`kiana-daemon/src/harness_sandbox.rs:bwrap_plan` 的 `read-only` 分支）。
- 可写沙箱（workspace-write）：项目根重新挂成可写，但**工作目录必须在项目根里面**，出了项目范围直接报错（同函数里的 `harness_workdir_outside_project`）。
- "完全放开、啥都不管"的档位（danger-full-access）**被明确拒绝**，入口层和沙箱层两层都拒（`kiana-entrypoints/src/harness_run.rs:sandbox_policy_from_name` 返回 `danger_full_access_rejected`；`bwrap_plan` 遇到未知档位返回 `sandbox_unsupported`）。
- 写盘必须同时满足：项目已信任 + 权限档不是最严的"安全档" + 角色本身允许写工作区，否则拒绝（`kiana-core/src/lib.rs:authorized_harness_sandbox`，拒绝码 `workspace_write_requires_trusted_non_safe_profile`、`role_sandbox_read_only`）。
- 两个并行任务抢同一片文件时，后来的拿不到锁，任务被拒，原因码 `path_lock_conflict`（`kiana-core/src/lib.rs:acquire_builder_path_locks`）。
- 项目本地的 skill（给 AI 看的说明书文件）和 plugin 目录，**只有项目受信才会被读取**（`kiana-skills/src/loader.rs:get_skill_dirs_with_trust`；`kiana-skills/src/plugins.rs:scoped_plugin_root_dirs`）。
- 旧版存在项目里的 `.kiana/trust.json` 被明确无视——它不是权威凭证（`kiana-types/src/trust.rs:read_project_trust` 及测试 `project_local_trust_file_cannot_authorize_project`）。
- 传进沙箱的环境变量走"白名单 + 拉黑"：只放行一小批基础变量，名字里带 KEY / SECRET / TOKEN 的一律剔除（`kiana-daemon/src/harness_sandbox.rs:sandbox_env`）。

**不能：**

- 没有"全权模式"。想绕过沙箱随便写、随便联网，目前代码里没有这条路，是硬拒的。
- 沙箱依赖 Linux 上的 bubblewrap（现成的进程隔离工具，命令名 `bwrap`）。机器上没装它，命令直接报错不执行，**不会退化成不隔离地裸跑**（`bwrap_plan` 里找不到就返回 `sandbox_unavailable:bwrap`）——这在别的系统上意味着目前没有可用的隔离手段（CURRENT_STATUS 未单独给这条评级，此处按代码现状描述）。
- 跨进程的文件锁在非 Unix 系统上**形同虚设**：`kiana-core/src/lib.rs:try_lock_path_file` 在非 Unix 下直接返回"锁成功"。
- 信任记录没有签名。任何能在你电脑上写你家目录的程序，都能改写它；记录里的项目路径校验只能防"张冠李戴"，防不了"本机上有权限的人"。
- CURRENT_STATUS.md §2 对"基础 trust / sandbox / role / path / memory policy"的评级是 **partial + local_behavior**，翻译成白话就是：代码在、在本机测过，还没当真产品验证过；同一行还明确记着"固定本地主体、角色/部门指派和持久身份仍未完成"。

## 代码怎么跑（走读）

### 第一幕：你敲 `kiana trust .`，写入信任记录

1. 命令进到 `kiana-commands/src/trust.rs` 的 `TrustCommand::execute`。它认好几种说法：`trust`、`.`、`trusted`、`allow` 都算"信任"；`untrust`、`reset` 算"撤销"；`status` 看当前状态。
2. "信任"最终走到 `kiana-types/src/trust.rs` 的 `write_project_trust`。它先做一件小事：从你当前所在目录**往上找 `.git` 文件夹**，找到的那个目录就算"项目根"（`project_trust_root`）——这样你在项目深处的子目录里敲命令，信任也是给整个项目办的。
3. 它给项目根算一个指纹：把项目路径做 SHA-256（一种把任意内容压成一串固定长度哈希的算法，同样路径永远得到同样哈希），这串哈希就是项目的 project_id（`project_trust_id_for_root`）。
4. 然后把一条记录写到 `~/.kiana/trust/projects/<project_id>.json`，内容大致是"project_id + 项目完整路径 + trusted: true"。**关键设计**：这条记录存在你家目录，不存在项目里。为什么？如果记录在项目里，别人发你一个恶意项目，项目里就可以自带一份"我可信"的记录，信任检查就形同虚设。代码里有测试专门验证项目里伪造的记录不算数。
5. 写入过程很谨慎：先落一个 pending 标记，再用"先写临时文件、一步替换到位"的方式落盘，最后撤掉标记。读取方只要看到 pending 标记还在，就宁可报错也不读半截记录（`persist_project_trust_record`、`reject_pending_project_trust`）。这是防"写到一半断电，留下残缺记录"。

### 第二幕：你敲 `kiana run --sandbox workspace-write -- "干点活"`，AI 要开工了

1. 入口 `kiana-entrypoints/src/harness_run.rs` 的 `client_on_host` 先干两件事：调 `project_trusted` 读信任记录；调 `sandbox_policy_from_options` 决定档位。你没写 `--sandbox` 就是只读档；写了 workspace-write 就是可写档；写了 danger-full-access 当场报错。另外 `kiana run` 默认只读，文件夹工作台默认可写，但**两个入口都要求项目已信任**。
2. `kiana-daemon/src/lib.rs` 的 `effective_permission_profile` 把档位翻译成"权限档"：只读档对应最严的 Safe（安全档），可写档对应 Balanced（均衡档）。权限档是后面策略判断的另一个旋钮。
3. 请求进入 `kiana-core` 的 ControlPlane（控制面）。ControlPlane 先问策略引擎——`kiana-policy/src/lib.rs` 的 `DefaultPolicyEngine::evaluate`。判断顺序本身是安全设计的一部分，从重到轻：
   - **第一问：项目可信吗？** 不可信，直接拒（`project_untrusted`），连"找人签字放行"的机会都不给。
   - **第二问：这个角色能干这事吗？** 每个角色（PM、Builder、Reviewer……）有一张固定的工具清单和路径清单，越界也是直接拒，同样不能靠人工批准翻案。
   - **第三问：这事敏感吗？** 名字里带支付、发布、删除、授权等字样的，至少要人工点一次头（Ask，"待审批"）。
   - **第四问：风险多大？** 只读的放行；本地写要看权限档；对外的副作用（比如调外部 MCP 工具）一律要审批。
4. ControlPlane 再过一遍沙箱档位本身——`kiana-core/src/lib.rs` 的 `authorized_harness_sandbox`：workspace-write 需要"已信任 + 非 Safe 档 + 角色允许写工作区"三个条件同时成立，否则拒绝。
5. 都通过后，模型说"我要跑条命令"，真正的执行在 `kiana-daemon/src/harness_capabilities.rs` 的 `sandboxed_command`：它调 `harness_sandbox.rs` 的 `bwrap_plan` 拿到一份 bwrap 参数列表（BwrapPlan），然后按这个列表启动 bubblewrap。列表里的关键几项：
   - 先把整个磁盘只读挂进隔离环境（`--ro-bind / /`）；
   - 再按档位重新挂项目根：只读档还是只读重挂，可写档改成可写重挂——**项目之外依然全部只读**；
   - 断网（隔离所有系统资源 `--unshare-all`）、清空进程权限、清空环境变量再只塞回白名单那几个；
   - 工作目录出了项目根，参数列表直接生成不出来，报错。

### 第三幕：两个任务同时开工，路径锁登场

1. 走 packet（工单）模式启动 Builder 时，`kiana-core/src/lib.rs` 的 run.spawn 处理流程会调 `acquire_builder_path_locks`。
2. 锁什么？锁的是**工单里声明要动的路径清单**（`kiana-domain` 的 `builder_lock_paths`：把清单去重排序；如果工单没写清单，就锁一个"全场独占"的万能锁 `EXCLUSIVE_PATH_LOCK`，谁也别想同时开工）。
3. 怎么算冲突？`kiana-domain` 的 `path_locks_conflict`：两条路径相同，或者一条是另一条的父目录，就算冲突——你锁了 `src/`，他想锁 `src/main.rs`，冲突。
4. 锁有两层：一层在 ControlPlane 内存里（同一进程内立刻生效）；一层在 `~/.kiana/locks/` 下落成真实文件、用操作系统的文件锁（flock）加锁，防的是两个**不同进程**同时开工。
5. 拿不到锁？任务直接被拒（`run.rejected`，原因 `path_lock_conflict`），并记进事件账本。拿到了锁？任务结束时有一个自动归还守卫（`BuilderPathLockGuard`），无论正常结束还是中途出错，锁都会被释放。

### 最重要的分支：为什么项目里的 skill 要过信任检查

skill 就是放在项目里给 AI 读的提示文件（`.claude/skills/`、`.kiana/skills/` 目录）。问题在于：**skill 文件也是项目内容，别人可以在项目里藏一份引导 AI 干坏事的 skill**。所以 `kiana-skills/src/loader.rs` 的 `get_skill_dirs_with_trust` 规定：你家目录里的 skill 随时可读，但项目里的 skill，只有这个项目被信任过才扫描加载。plugin（插件包）同理（`plugins.rs` 的 `scoped_plugin_root_dirs`）。这也是信任记录存家目录的同一个逻辑：**不让"被检查的东西"自己决定自己可不可信**。

### 失败时怎么走

信任记录读不出来（文件被改坏、路径对不上、pending 标记还在）一律按"未知/不可信"处理，宁可误伤也不放行——代码里管这叫 fail-closed（出了故障朝"关门"的方向倒）。CURRENT_STATUS.md 里大量"…fail-closed evidence"的记录，说的就是这类行为的回归测试。

## 关键概念速查

| 概念 | 白话解释 | 代码在哪 |
|---|---|---|
| ProjectTrust | 项目信任状态：三态——可信 / 不可信 / 未记录（未记录按不可信对待） | `kiana-types/src/trust.rs` |
| trust 记录 | 记录本体，存 `~/.kiana/trust/projects/<指纹>.json`；带 schema 版本、project_id、项目路径、可信与否四个字段，多余字段直接报错 | `kiana-types/src/trust.rs:ProjectTrustRecord` |
| 项目指纹（project_id） | 项目路径的 SHA-256 哈希，同路径永远同指纹 | `kiana-types/src/trust.rs:project_trust_id_for_root` |
| 沙箱档位 | `read-only`（只读）和 `workspace-write`（项目内可写）两档，都要求已信任；`kiana run` 默认只读、文件夹工作台默认可写 | `kiana-entrypoints/src/harness_run.rs:sandbox_policy_from_name` |
| bubblewrap / bwrap | 现成的 Linux 隔离工具，负责把命令放进隔离环境执行；没装就报错不执行 | `kiana-daemon/src/harness_sandbox.rs:bwrap_plan` |
| 权限档（permission profile） | Safe / Balanced 等档位，决定"本地写"要不要审批；只读档对应 Safe | `kiana-daemon/src/lib.rs:effective_permission_profile` |
| RoleSpec | 角色规格：每种角色能用哪些工具、能写哪些路径，写死在代码里的清单 | `kiana-domain`；策略层用它的地方在 `kiana-policy/src/lib.rs:role_decision` |
| 路径锁 | 工单声明要动的路径清单的互斥登记，内存 + 文件两层 | `kiana-core/src/lib.rs:acquire_builder_path_locks` |
| 全场独占锁 | 工单没写路径清单时锁上的"万能锁"，其他任何任务都得等 | `kiana-domain` 的 `EXCLUSIVE_PATH_LOCK`（用法见 `builder_lock_paths`） |
| fail-closed | 出故障时朝"关门"方向倒：读不出卡当没卡，锁不上当冲突 | 各处错误分支，如 `kiana-types/src/trust.rs:read_project_trust` |
| 授权 ID（authorization_id） | 策略放行时生成的授权标识，Gate 只认可带它的放行 | `kiana-policy/src/lib.rs` 发，`kiana-gates/src/lib.rs:DefaultGateEngine::evaluate` 验 |

## 设计视角：现在最明显的短板

以下只说现状，不开药方：

1. **跨进程文件锁在非 Unix 系统上是空转的**：`kiana-core/src/lib.rs:try_lock_path_file` 在非 Unix 下无条件返回"锁成功"，两个不同进程可以同时拿同一把锁（同进程内的内存锁仍然有效）。
2. **隔离手段只有 Linux 上的 bubblewrap 一种**：没装 bwrap 的机器（包括 macOS、Windows）上，shell 执行是直接报错而不是换一种方式隔离——安全上是对的（宁可不跑），但也意味着这些平台目前没有可用沙箱。
3. **路径锁锁的是"工单声明的清单"，不是"实际写的动作"**：shell 命令实际改了哪些文件，锁本身不知道，靠沙箱的"项目内才可写"边界兜底；锁只负责防两个任务撞车。
4. **信任记录无签名、无完整性保护**：本机上任何能写你家目录的程序都能改写信任记录；记录校验只防"路径对不上号"这种错位，防不了本机权限。
5. **整个领域停留在 partial + local_behavior**（CURRENT_STATUS.md §2）：本机、假模型脚本条件下验证过；"持久身份"没做完意味着"谁在用系统"目前是写死的本地用户，角色/部门指派也不持久。

## 相关文档

- `docs/company-os-overview.md`：词条 "trust"（项目信任）、"sandbox"（沙箱档位）、"RoleSpec"（角色规格）；"常见翻车现场"表中"项目没 trust 就要写盘"一行。
- `CURRENT_STATUS.md` §2 能力表："基础 trust / sandbox / role / path / memory policy"一行（partial + local_behavior）。
- `docs/features/` 兄弟文档：02（工具调用与授权脊柱，沙箱档位在 shell 执行时还会二次校验）、05（审批）、07（角色与工单 path_allow 的衔接）；全部篇目见 `docs/features/README.md` 索引。
