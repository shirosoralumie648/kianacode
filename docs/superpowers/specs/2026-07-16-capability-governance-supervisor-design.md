# Capability Governance Rust Supervisor 权威边界设计

日期：2026-07-16

执行规则：本 remediation 不采用 TDD。先实现批准的 Rust supervisor 合同，再添加并运行 focused、adversarial、integration 和 review 验证；不要求 pre-implementation failure run。

## 1. 背景

Phase 1 Plan 01-03 的 `scripts/capability-governance-smoke.sh` 同时承担公开 CLI、worker、deadline、network guard、trace parser、output publisher 和 cleanup。连续审查已证明这种 Shell 内多权威结构会 fail open：trace parser 返回未枚举状态时，parent 只拒绝已知错误码，随后仍可发布 worker 的 `OK` 并返回 `0`。

本设计不再继续修补 Shell 状态分支，而是建立一个专用 Rust supervisor。公开 Bash CLI 和 fixture semantic checks 保留；所有运行时成功权统一移入一个小型、可独立测试的进程。

现有 `01-03-SUMMARY.md` 的 `status: complete` 和 runtime pass claims 已被后续 authority review 否定。在本 remediation 完成并通过独立复审前，该 summary 不是推进依据，`01-04` 不得启动；最终实现必须原位重写其 runtime claims 和验证证据。

## 2. 已批准的范围变更

原 01-03 计划要求 Bash/Python runner 无需 Cargo build 即可运行。该约束被以下新合同替换：

1. 允许在 workspace root 运行 `env -u CARGO_BUILD_TARGET cargo build --target-dir "$PWD/target" -p kiana-capability-governance-supervisor --locked --offline` 构建专用 helper。
2. 不提交预编译二进制。
3. fresh source archive 必须能在预热 Cargo cache 和 Rust 1.96 环境中离线构建并执行。
4. `<30s` 约束从 Cargo build 完成后开始，只约束单个 `schemas` 或 `fixture-shapes` slice。
5. 01-03 PLAN 的 `must_haves`、文件列表、动作和验证命令必须在实现前同步更新，不能保留与 Rust helper 冲突的 no-Cargo 声明。

本设计仍禁止 pip、npm、网络安装和完整产品构建作为 focused gate 的前置条件。

## 3. 目标

1. 只有一个 supervisor 能生成公开成功状态和最终 `offline=true`。
2. caller environment、worker output、receipt、launcher exit 或 parser 异常均不能单独产生成功。
3. worker 及其全部后代在 network namespace 中运行；worker 不继承 network/io_uring FD，所有 `%network` 与 io_uring syscall 都被阻断、记录并使 slice 失败。
4. supervisor 自己持有 monotonic deadline、进程树回收、输出隔离、trace 解析和 cleanup。
5. 正常两个 slice 在构建后各自小于 30 秒，并继续支持无 `reference/` 的 tracked source archive。
6. 回归测试位于独立 test surface；生产 runner 不再包含环境变量触发的成功或 review short-circuit。

## 4. 非目标

- 本切片不实现 macOS sandbox 或 Windows Job Object adapter。
- 本切片不实现直接 `clone3`、pidfd、seccomp USER_NOTIF 或 cgroup v2 supervisor。
- 本切片不把 supervisor 打入 Kiana 产品发行包。
- 本切片不修改四份 valid fixture 的治理语义或内容。
- 本切片不保护能够修改 kernel、检出源码、构建产物或 root-owned launcher 的攻击者。
- 本切片不把 launch token/receipt 宣称为 caller authentication；它们仅提供同一次 launch 的 correlation 和 liveness。

## 5. 可信计算基

成功判定依赖以下对象：

- Linux/WSL kernel 与 user namespace 能力；
- 已检出的 Bash runner 和 Rust supervisor 源码；
- 按 locked/offline workspace 构建出的 supervisor binary；
- supervisor build 时验证并嵌入的 canonical Python interpreter；
- 固定路径 `/usr/bin/bwrap`；
- 固定路径 `/usr/bin/strace`；
- 固定路径 `/usr/bin/bash`；
- canonical `/bin/sh`（仅供 strace 启动固定 trace logger command）；
- supervisor 创建并独占的 runtime root 和 inherited file descriptors。

`/usr/bin/bwrap`、`/usr/bin/strace`、`/usr/bin/bash` 与 canonical `/bin/sh` 每次运行都必须满足：regular file、uid/gid 为 root、group/other 不可写、owner executable。前三者 canonicalize 后仍必须等于固定路径；`/bin/sh` 允许 canonicalize 到 root-owned system shell。任一条件失败时返回 `launcher_untrusted`，不得回退到 PATH、配置路径或弱隔离。

当前 `/usr/bin/python3` 不含本 runner 已依赖的 `jsonschema`，因此 runtime 不能简单固定到 system Python。新 crate 使用一个零依赖 `build.rs`：构建时从构建环境解析 `python3`/`python`，canonicalize，要求 regular executable、owner 为 build uid 或 root、other 不可写；group-write 仅在 interpreter gid 等于 build gid 时允许，因为 build 用户环境已属于 TCB。build script 用 isolated mode (`-I`) 对一个已知 positive 和一个已知 negative Draft 2020-12 contract 执行 `jsonschema` canary；随后通过 compile-time env 将 exact path 嵌入 supervisor。`build.rs` 声明 `rerun-if-env-changed=PATH`。runtime PATH 或 caller env 不能重新选择 Python；嵌入路径失效或 preflight 不再通过时 fail closed，要求重新构建。

caller 可以控制公开 argv、普通环境变量和 cwd，但 production binary 使用 build-time canonical repository root，不能由 cwd 选择 runner；runtime `PATH`、`BASH_ENV`、`ENV`、`PYTHONPATH` 或 `PYTHONHOME` 也不能替换可信对象。supervisor 启动 child 时使用清空后的环境，只恢复固定必要值和嵌入的 Python path。动态链接器在 Bash/Rust 代码运行前处理 `LD_PRELOAD`/`LD_LIBRARY_PATH`，因此同 UID 的 pre-exec loader injection 明确属于 trusted-launcher/host boundary，必须由调用方使用 sanitized `execve` 环境保证，不能声称由已被装载的程序自身消毒。能够控制 Cargo build 环境、构建产物或嵌入解释器的主体同样位于本门禁的防御范围之外。

## 6. 组件边界

### 6.1 公开 Bash CLI

`scripts/capability-governance-smoke.sh` 继续只公开：

```text
scripts/capability-governance-smoke.sh {schemas|fixture-shapes}
```

缺失或未知 slice 返回 `2`。公开模式只做参数和固定 helper 路径检查，然后 `exec`：

```text
<repo>/target/debug/kiana-capability-governance-supervisor
```

runner 不执行隐式 Cargo build，不读取环境变量选择 supervisor，不从 PATH 搜索 helper。01-03 focused gate 固定使用 workspace 默认 `target/debug`；custom `CARGO_TARGET_DIR` 不属于该 gate 合同。helper 缺失时输出稳定的 build prerequisite 诊断并返回 `1`。

### 6.2 Rust supervisor crate

新增独立 workspace binary crate：

```text
kiana-capability-governance-supervisor/
  Cargo.toml
  build.rs
  src/lib.rs
  src/main.rs
  tests/supervisor_linux.rs
```

crate 为 `publish = false`，仅依赖 `std + libc`。`build.rs` 也只使用标准库。不依赖 `kiana-tools`、Tokio、Clap、Serde、nix、rustix 或 tempfile，避免把产品 tool graph 拉入 focused gate。

`src/lib.rs` 拥有 typed state、launcher validation、bounded capture、deadline、signal/process cleanup、trace parser、receipt parser、trusted worker-launcher 和唯一成功谓词。`src/main.rs` 只解析固定内部参数并映射公开退出码。crate 在非 Linux target 编译为只返回 `supervisor_unavailable` 的 fail-closed stub，不把 Linux-only syscall surface 扩散到 macOS/Windows workspace build。

### 6.3 Hidden Bash worker

supervisor 在 sandbox 内调用 runner 的 hidden worker mode。该模式不是第二个公开成功入口：

- Rust worker-launcher 必须消费 inherited launch FD；
- 必须先完成 startup socket canary；
- 只能运行已知 slice；
- launch/completion FD 不传给 semantic Bash；semantic 成功使用保留码 `80`，普通 `0` 不算成功；
- Rust worker-launcher 只在 semantic child 已退出 `80` 后写 exact receipt，并以 `80` 退出；
- 永不打印最终 `offline=true`；
- direct worker invocation 必须非零且不能形成公开成功输出。

现有 schema/fixture semantic functions 保持在 worker 中，避免本切片重写约 1,000 行已验证的 fixture logic。

## 7. Linux MVP adapter

supervisor 固定构造：

```text
Rust supervisor
  -> /usr/bin/bwrap
       --die-with-parent
       --unshare-all
       --clearenv
       --ro-bind / /
       --dev /dev
       --tmpfs /proc
       --remount-ro /proc
       --tmpfs /tmp
       --dir /tmp/kiana-home
       --ro-bind <canonical-supervisor-bin> /tmp/kiana-supervisor
       --bind <canonical-worker-tmp> /tmp/kiana-worker
       --chdir <canonical-repo-root>
       --setenv PATH /usr/bin:/bin
       --setenv LC_ALL C
       --setenv HOME /tmp/kiana-home
       --setenv TMPDIR /tmp/kiana-worker
       --setenv KIANA_GOVERNANCE_PYTHON <embedded-python-path>
       --setenv KIANA_GOVERNANCE_LAUNCH_FD <launch-fd-for-Rust-launcher>
       --setenv KIANA_GOVERNANCE_COMPLETION_FD <completion-fd-for-Rust-launcher>
       --setenv KIANA_GOVERNANCE_WORKER_STDOUT_FD <worker-stdout-fd>
       --setenv KIANA_GOVERNANCE_WORKER_STDERR_FD <worker-stderr-fd>
    -> /usr/bin/strace -f -qq
         -e trace=%network,io_uring_setup,io_uring_enter,io_uring_register
         -e signal=none
         -e inject=%network:error=EPERM
         -e inject=io_uring_setup:error=EPERM
         -e inject=io_uring_enter:error=EPERM
         -e inject=io_uring_register:error=EPERM
         -o '|/tmp/kiana-supervisor __trace-logger'
         -- /tmp/kiana-supervisor __worker-launcher
              --runner <canonical-runner>
              --slice <schemas-or-fixture-shapes>
        -> consume launch token + retain completion FD as CLOEXEC
        -> spawn /usr/bin/bash <runner> --internal-worker <slice>
        -> after Bash exit 80, write receipt and exit 80
```

所有 angle-bracket 值都由 supervisor 从 canonical paths、固定 slice enum 和新建 FD 中生成，并作为 argv/env 的独立参数传给 `Command`/bwrap；不得通过 shell 拼接。唯一由 strace pipe syntax 交给 `/bin/sh` 的字符串是完全固定的 trace-logger command。

`bwrap` 提供独立 user/PID/network/mount namespace、private `/dev` 和 tmpfs `/tmp`。host procfs 被空的 read-only tmpfs 覆盖；worker 不需要 procfs，不能经 `/proc/<pid>/fd` 或 `/dev/fd` 重开 strace/logger channel。项目树只读。supervisor binary 被 read-only bind 到固定、无 shell metacharacter 的 `/tmp/kiana-supervisor`；worker tmp directory 单独 bind，整个 runtime root 不暴露给 child。

strace output-pipe 只执行固定命令 `/tmp/kiana-supervisor __trace-logger`。trusted logger 从 strace-owned stdin 读取 trace，并将原始 bytes 写入 bwrap stdout；该 stdout 在 parent 侧连接独立 bounded trace pipe。trusted `__worker-launcher` 在 spawn Bash 前设置 `PR_SET_NO_NEW_PRIVS`、清空 capabilities，把 stdout/stderr 切换到另外两组 worker pipes，并用 `close_range` 或等价 bounded fallback 关闭原 trace stdout、guard stderr 和所有 unexpected FD。它先消费 launch token，并将 completion FD 设为 `CLOEXEC`，因此 Bash worker 没有 launch/completion authority FD、trace path、logger stdin 或 parent trace pipe。Rust launcher 只在 Bash 退出 `80` 后写 receipt。

strace pipe command 是固定常量，不包含 repo path、slice 或 caller data，避免 shell interpolation。logger 不读取环境选择目的地，不写 filesystem，也不生成 network syscall。任何 logger byte、EOF、exit 或 channel 异常都使 trace verdict 失败。

`--die-with-parent` 和 PID namespace 与 supervisor process-group cleanup 共同保证后代回收。真实 Linux integration test 必须证明 worker 找不到 trace path/FD，并且向所有 inherited FD 写入都不能修改 parent trace capture。

`strace` 不是最终成功 authority。它只提供两项能力：

1. 对 `%network` syscall 注入 `EPERM`；
2. 生成 supervisor 在进程内解析的 audit trace。

worker 的第一条 network syscall 是固定 startup `socket` canary，且必须观察到 `EPERM`。trace 必须恰好包含这一条 injected canary；任何额外 `%network` 或 io_uring syscall 都产生 `network_attempt` 并失败。

worker-launcher 以 script-file 形式 spawn `/usr/bin/bash`，stdin 固定为 `/dev/null`，不得改用会产生额外 shell startup network probe 的 `bash -c` 路径。真实 integration canary 必须证明最终 trace 只有一条 startup socket；设计不通过宽泛 allowlist 忽略 `getpeername` 等额外调用。

本合同不错误声称 `ioctl` 属于 strace `%network`。worker 启动前关闭全部 inherited socket/io_uring FD，private `/dev` 不暴露 tun 等 host network device，且 io_uring setup/enter/register 被显式注入 `EPERM`。因此 worker 可用的 network-capable path 要么先经过被审计的 `%network` syscall，要么经过已禁用的 io_uring。network namespace 是最终不可触达 host network 的 enforcement；trace 是“出现被支持接口的尝试则整个 slice 失败”的 audit authority。

直接 seccomp USER_NOTIF/pidfd adapter 留在后续演进。原因是普通 seccomp ERRNO/KILL 只能证明不可联网，不能证明一个被 shell 忽略的后代网络尝试必然使整个 slice 失败；USER_NOTIF listener 会把 01-03 扩大为独立内核沙箱项目。MVP 明确禁止 inherited network/io_uring FD，并仅对上述可观测接口作 attempt 断言。专用 Rust crate 保留 adapter 边界，以后可以替换 bwrap/strace 而不改变公开 CLI 或成功合同。

## 8. 数据流

1. supervisor canonicalize project root、runner 和固定 launcher。
2. 忽略 caller `TMPDIR`，在固定 `/tmp` 下用随机 token 和 create-if-absent 语义创建 mode `0700` 的 runtime root；已存在、symlink 或 ownership/mode 不匹配均重新取样或失败。
3. 通过 `getrandom` 生成 32-byte launch token。
4. 创建 one-use launch pipe 和 completion pipe；除明确传递的 control/stdout/stderr FD 外，关闭或设置 `CLOEXEC`，不得把已有 socket 或 io_uring FD 带入 sandbox；worker 可见 procfs 保持为空。
5. 创建相互独立的 worker stdout、worker stderr、guard stderr 和 parent trace pipes，以及独立 worker tmp directory；child 输出在验证前不可见。
6. 清空 child environment，写入固定 `PATH=/usr/bin:/bin`、`LC_ALL=C`、private `HOME`、`TMPDIR`、嵌入的 canonical Python path 及 FD 编号。
7. 以独立 process group 启动 bwrap/strace chain，并并发有界读取 stdout/stderr。
8. trusted trace-logger 将 strace stream 送入 parent trace pipe；trusted worker-launcher 消费 launch token、保留 `CLOEXEC` completion FD、重定向 worker output、关闭 trace/guard/unexpected FD，再 spawn Bash。
9. supervisor 用 monotonic clock 等待完成；deadline 为 30 秒。
10. Bash worker 运行 canary 和 semantic slice 并以 `80` 退出；trusted Rust worker-launcher 随后写 exact receipt 并同样以 `80` 退出。
11. supervisor wait/reap、解析 trace 和 receipt、检查输出、检查后代和 worker tmp。
12. supervisor 将所有结果归并为一个 typed verdict。
13. 成功时先将已捕获输出读入内存，再删除并确认 runtime root 不存在，最后发布内部校验输出和唯一 final marker。

receipt 格式固定为：

```text
complete:<64 lowercase hex token>:<slice>\n
```

必须精确匹配、只出现一次，并随后 EOF。提前、重复、缺失、超长或 trailing bytes 全部失败。

## 9. 唯一成功谓词

公开退出 `0` 只允许在以下全部为真时产生：

```text
launchers_trusted
&& sandbox_started
&& deadline_state == completed
&& worker_exit == 80
&& trace_channel == parent_owned_and_worker_inaccessible
&& trace_verdict == exact_startup_canary_only
&& receipt_verdict == exact_and_eof
&& stdout_capture == complete_and_bounded
&& stderr_capture == empty_and_bounded
&& worker_stdout_has_no_final_marker
&& process_tree == fully_reaped
&& worker_tmp == empty
&& runtime_cleanup == removed
```

实现必须以正向合取计算成功。不得通过枚举已知失败码后 fall through，也不得把 unknown variant 映射为 success。

worker exit `0` 是错误而不是成功，因为 no-op launcher 或错误 wrapper 最容易产生 `0`。只有保留码 `80` 可表示 worker semantic success，且它仍不能绕过其余谓词。

## 10. 有界资源

- slice deadline：30 秒 monotonic；
- TERM grace：2 秒，随后 KILL process group；
- stdout：最大 1 MiB；
- stderr：最大 1 MiB；
- trace：最大 64 KiB，超过即终止并失败；
- receipt：最大 256 bytes；
- runtime root：只允许 supervisor bookkeeping 和一个独立 worker tmp subtree；trace 只存在于 parent pipe/in-memory capture，不落入 worker 可见 filesystem。

reader 必须并发 drain stdout/stderr，避免 pipe backpressure 伪装成 timeout。supervisor 在 wait loop 中同时监控 trace size；超过上限立即进入统一 cleanup。任何截断都视为失败，不能在截断输出上宣布成功。

## 11. 失败与输出语义

公开退出码：

- `0`：唯一完整成功；
- `1`：semantic、launcher、sandbox、network、deadline、receipt、output、process 或 cleanup 失败；
- `2`：公开 CLI usage；
- `130`：supervisor 收到 SIGINT，完成回收后退出；
- `143`：supervisor 收到 SIGTERM，完成回收后退出。

authority 未成立时，worker stdout/stderr 和 raw trace 均不得发布。supervisor 只输出稳定、脱敏代码，例如：

- `supervisor_unavailable`
- `launcher_untrusted`
- `sandbox_start_failed`
- `sandbox_attestation_failed`
- `network_attempt: blocked syscall=<name>`
- `deadline_exceeded`
- `worker_failed`
- `receipt_invalid`
- `output_limit_exceeded`
- `process_cleanup_failed`
- `runtime_cleanup_failed`

诊断不得包含 nonce、raw syscall 参数、PID、absolute home path 或 runtime path。

当 sandbox/trace authority 已成立、没有 network attempt，但 semantic worker 非成功时，可以发布受限的 schema/fixture 诊断，以保留 `schema_validation_failed: keyword=uniqueItems` 等测试价值。semantic stderr 仍需有界且禁止 final marker、secret-shaped value 和 transient path。

## 12. 信号与后代清理

supervisor 使用独立 process group 启动 bwrap。正常 deadline、SIGINT、SIGTERM 或内部错误均执行同一 bounded cleanup：

1. 向 process group 发送 TERM；
2. 在 grace 内循环 `try_wait`；
3. grace 到期向 process group 发送 KILL；
4. wait direct child；
5. 依靠 bwrap PID namespace 的 PID 1 退出清理 namespace descendants；
6. drain/关闭所有 pipe；
7. 删除 runtime root 并确认路径不存在。

process group 不是单独的完整安全边界；它与 bwrap PID namespace、`--die-with-parent` 共同组成 MVP 回收合同。任何 cleanup syscall 或验证失败都覆盖 worker success 并返回 `1`。

## 13. 测试设计

### 13.1 Pure state tests

在 `src/lib.rs` 对 verdict state 做表驱动测试：逐一改变 worker exit、deadline、trace、receipt、stdout、stderr、process tree 和 cleanup 状态，证明除唯一全真组合外全部失败。显式覆盖 worker exit `0`、未知状态和未来新增 enum variant 的 fail-closed default。

### 13.2 Fake adapter tests

test-only dependency injection 使用临时 executable 模拟：

- no-op/rejected launcher；
- launcher 返回 `0` 但未启动 worker；
- 未知 launcher/worker exit；
- missing、duplicate、wrong-token、wrong-slice、trailing receipt，以及 semantic child 无 receipt-FD capability；
- stdout 伪造 final marker；
- stdout/stderr/trace 超限；
- worker 尝试发现 trace path/FD 或向 inherited FD 伪造 trace；
- worker-launcher 未重定向 output 或泄漏 trace/guard FD；
- deadline hang；
- TERM 后不退出；
- descendant/process-group cleanup；
- runtime cleanup failure。

生产 binary 不读取环境变量选择这些 adapters。

### 13.3 Real Linux integration tests

真实 `/usr/bin/bwrap` 与 `/usr/bin/strace` 覆盖：

- 两个正常 slice；
- startup canary；
- 额外 socket/network syscall；
- io_uring setup/enter/register；
- trace channel 对 worker 不可见且不可篡改；
- `/proc/<pid>/fd`、`/dev/fd` 与 unexpected inherited FD 不能重开 trace channel；
- worker hang；
- direct hidden worker；
- SIGINT=130、SIGTERM=143；
- child、runtime root 和 caller TMPDIR 零残留；
- caller cwd、PATH、BASH_ENV 与 PYTHONPATH 不改变 build-bound repo、runner 或可信 launcher；
- raw trace 不进入公开诊断。

### 13.4 Independent shell gate

新增 `scripts/capability-governance-supervisor-smoke.sh` 承接原生产脚本中的 review regressions。生产 runner 不再识别 `KIANA_CAPABILITY_GOVERNANCE_REVIEW_TEST` 或任何同类成功 short-circuit。

### 13.5 Fresh archive gate

fresh gate 使用一个明确的 committed/alternate-index tree，绝不归档整个 dirty working tree：

1. 批准的实现与实现后验证可以在 live tree 运行。
2. final pre-commit fresh gate 创建临时 `GIT_INDEX_FILE`，从 `HEAD` 执行 `read-tree`，只加入 supervisor scoped paths；overlapping `Cargo.lock` 仅应用 supervisor-owned patch，然后用 `write-tree` 得到 tree object。
3. 从该 tree object 执行 `git archive`；archive 不包含用户其他 dirty 文件或 ignored `reference/`。
4. code/test commits 完成后，用 `git archive HEAD` 再跑同一确认；通过后才提交最终 SUMMARY。
5. 最终 SUMMARY commit 完成后，对最终交付 `HEAD` 再运行一次同样的 archive build/two-slice gate。该 post-summary verification 不再修改仓库；输出作为 handoff/phase verification evidence，避免验证后再改变 HEAD 的循环。

在 snapshot 中运行：

```bash
env -u CARGO_BUILD_TARGET \
  cargo build --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline
bash scripts/capability-governance-smoke.sh schemas
bash scripts/capability-governance-smoke.sh fixture-shapes
```

前置条件明确为 Rust 1.96、预热 Cargo cache，以及 build 环境中一个 isolated mode 可导入 `jsonschema` 的 Python。`build.rs` 将该解释器绑定到产物，runtime 不再从 PATH 选择。cache-empty airgap 不在 01-03 的 fresh 定义中；如果未来要求零 cache，需要 vendor 或 std-only raw FFI 的独立设计。

## 14. 验证命令

focused gate：

```bash
cargo fmt --all --check
env -u CARGO_BUILD_TARGET \
  cargo test --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline --no-fail-fast
env -u CARGO_BUILD_TARGET \
  cargo build --target-dir "$PWD/target" \
  -p kiana-capability-governance-supervisor --locked --offline
bash scripts/capability-governance-supervisor-smoke.sh
bash scripts/capability-governance-smoke.sh schemas
bash scripts/capability-governance-smoke.sh fixture-shapes
```

完成 focused gate 后，再在隔离 HOME/TMPDIR 和串行 test threads 下运行 workspace gate。完整 workspace gate 不替代 focused adversarial gate。

isolated workspace gate 固定为：

```bash
TEST_ROOT="$(mktemp -d /var/tmp/kiana-supervisor-gate-XXXXXX)"
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT/home" "$TEST_ROOT/tmp"
HOME="$TEST_ROOT/home" \
USERPROFILE="$TEST_ROOT/home" \
TMPDIR="$TEST_ROOT/tmp" \
CARGO_HOME=/home/shirosora/.cargo \
RUSTUP_HOME=/home/shirosora/.rustup \
cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1
```

这条命令是当前 checkout 的 Phase 1 gate；换 runner 时只能替换显式 Cargo/Rustup cache paths，隔离 HOME、USERPROFILE、TMPDIR 与串行 threads 的语义不变。

## 15. 文件与 dirty-worktree 边界

预期实现文件：

- `Cargo.toml`
- `Cargo.lock`
- `kiana-capability-governance-supervisor/Cargo.toml`
- `kiana-capability-governance-supervisor/build.rs`
- `kiana-capability-governance-supervisor/src/lib.rs`
- `kiana-capability-governance-supervisor/src/main.rs`
- `kiana-capability-governance-supervisor/tests/supervisor_linux.rs`
- `scripts/capability-governance-smoke.sh`
- `scripts/capability-governance-supervisor-smoke.sh`
- `.planning/phases/01-baseline-evidence-governance/01-03-PLAN.md`
- `.planning/phases/01-baseline-evidence-governance/01-03-SUMMARY.md`

`Cargo.lock` 已有用户未提交内容。实现只能基于 live lockfile 生成新 workspace package/dependency 关系，不得整文件恢复、覆盖或重生成丢失用户修改。四份 valid fixture、`.planning/config.json`、`scripts/schema-contract-smoke.sh` 和 `scripts/validate-json-schema.py` 保持保护状态。

涉及 `Cargo.lock` 的 commit 和 fresh tree 必须使用 resetless alternate-index/partial-patch 协议：临时 index 从 `HEAD` 初始化，仅向其应用 supervisor-owned lock hunk；这类操作完全不触碰真实 index，live worktree 保留“用户 hunk + supervisor hunk”。提交后确认 HEAD 只获得 supervisor lock relation，而 worktree 相对新 HEAD 仍保留原用户 hunk。禁止 `git add -- Cargo.lock`。

GSD `has_summary` 当前只看文件存在，不能理解 blocked frontmatter。remediation 期间禁止调用 `$gsd-execute-phase 1` 自动路由；实现直接从本设计经 `superpowers:writing-plans` 生成的专用计划执行，绕过 phase-plan-index。保持 `01-04` 禁止启动，直到 summary 被最终通过的证据原位更新；恢复 `complete` 后才重新进入正常 GSD phase routing。

不含 overlapping dirty 的普通 commit 可以使用真实 index 逐路径 stage，但提交前必须确认 cached 列表恰好等于声明路径，提交后真实 index 必须恢复为空。包含 `Cargo.lock` 的 commit 和所有 fresh-tree 组装只走上述 alternate index，期间真实 index 始终不变。禁止 `git add .`、`git add -A` 或整文件 stage `Cargo.lock`。

## 16. Partial Implementation 迁移

保留概念：

- parent-owned runtime root；
- stdout/stderr quarantine；
- worker tmp containment；
- exact completion receipt；
- startup socket canary；
- signal与正常退出共享 cleanup；
- 生产入口禁用 review success short-circuit；
- seed-first archive portability。

废弃实现：

- Shell 内逐项枚举 parser failure status；
- 外部 `timeout` 作为 deadline authority；
- nonce/FD 是 caller authentication 的表述；
- worker 铸造最终 `offline=true`；
- 环境变量选择 regression runner；
- worker exit `0` 被 parent 直接视为成功。

Failure-evidence commit `a9e4a11` 保留为架构失败证据。当前未提交 partial implementation 不作为一个可发布 commit。

实现的第一个原子 docs task 必须：

1. 将 `01-03-SUMMARY.md` 从 `status: complete` 降为 `status: blocked`；
2. 将受 runtime authority 影响的 pass claims 标记为 blocked/superseded，并记录本设计路径与 `a9e4a11`；
3. 修改 `01-03-PLAN.md`，用已批准的 Rust build/runtime 合同替换 no-Cargo 文件、must-have 和验证声明。

这个 demotion commit 在任何新 production code 前完成。之后按本设计替换 supervisor 部分；只有新 Rust 实现及其实现后验证、fresh gate 和独立质量复审全部通过，才原位恢复 summary `status: complete` 并写入新的 runtime evidence。

## 17. 验收标准

1. public success 只能由 Rust supervisor 的唯一正向谓词产生。
2. parser、launcher、worker、receipt、deadline、output、process 和 cleanup 任一未知状态均 fail closed。
3. direct worker、no-op launcher、worker exit `0` 和伪造 final marker 均不能产生 public `0`。
4. bwrap/strace 路径和 ownership 不满足合同即失败，无 PATH 或弱隔离 fallback。
5. build-time canonical Python/repository 绑定有效；runtime cwd/PATH 不能替换 runner/interpreter，缺失 `jsonschema` 时构建或 preflight fail closed；pre-exec dynamic-loader injection 属于调用方 sanitized-exec 前置条件，不计入进程内防御声明。
6. trace channel 由 strace logger 和 parent 独占；worker 无 path/FD 能力修改它，篡改探针稳定失败。
7. worker 不继承 network/io_uring FD；任一额外 `%network` 或 io_uring syscall 被阻断并使整个 slice 失败，network namespace 保证无法触达 host network。
8. timeout、SIGINT、SIGTERM 后无 child、worker tmp、runtime root 或 caller TMPDIR residue。
9. authority 失败不泄漏 raw trace、nonce、PID 或 transient/absolute path。
10. 两个正常 slice 在预构建后各自小于 30 秒。
11. fresh scoped tree 和 committed HEAD archive 在预热 Cargo cache 和受支持 Python build 环境下均可 locked/offline build 并运行，无 `reference/` 依赖。
12. 四份 valid fixture byte-for-byte 不变，用户 dirty files 未被覆盖或混入提交。
13. SUMMARY 在 code 前先 demote；focused adversarial gate、Rust tests、format、isolated workspace gate 和独立质量复审全部通过后，01-03 才能恢复 `complete`。
