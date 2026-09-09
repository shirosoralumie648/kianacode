# Kiana

Local-first agent **company OS**: PMP process groups are departments, roles have independent context/prompts/ACL, departments may hold bounded symposiums, memory is layered RAG.

v1.0 已在 `local_behavior` 上限下勾选：temp `INSTALL_DIR` 生命周期 + [USER.md](USER.md) + P0 审计 + [NOTICE](NOTICE)。v1.0.4 工作台会话面（TTY conversation / input / status，同一 `DaemonHost`）已绿；`kiana tui` 仍 park。token 流式已接入 CLI / 文件夹工作台 / loopback Web（SSE）三面，逐块交付有 DeepSeek 实跑证据，开关为 `KIANA_STREAMING=off|auto|on`（默认 `auto`）；live provider 仅覆盖已实跑的 DeepSeek 兼容端点（单 provider、单模型），未验证原生 OpenAI/Anthropic 凭据。这不是 `~/.local/bin` 生产安装、不是签名包、不是企业、不是 dsh Web UI 克隆。v0.6.1 ORCH-04（同核两个 packet Builder + 路径锁；越权 `packet_path_denied`）仍必须绿。v0.5 later SYMP-04（五部门有界会；规划会回归不破；联席冻结）仍必须绿。v0.5 later LONG-02（超预算 compact 进收据；同核 continue 之后仍能写盘；pause 仍是 cancel）仍必须绿。v0.5.2 六层记忆 ACL 与 v0.5.1 五个部门对象仍必须绿。v0.4 Coding pack 仍必须绿（P1-READ skipped：搜索走 `shell`）。v0.3 规划部/执行部/有界会 + cassette eval/install 仍必须绿。v0.2 黄金路径仍必须绿（`kiana run` / print → daemon → `KianaHarness`，默认工人是执行部 Builder）。`kiana tui` **park**：仍走 legacy SDK/stream，不是产品路径。Daily/Research 仍草案。不要打开 v1.x / worktree / SDK/IDE / JointSymposium。

从 0 到完整产品的方案（按 `reference/` 中 coding agent 的 git 顺序 + 公司编制）在：

- [COMPANY.md](COMPANY.md) — 五部门、角色、有界群聊、六层 RAG/ACL、两个 Archon 怎么用
- [DESIGN.md](DESIGN.md) — 代码真相、版本阶梯、冻结、映射
- [PROCESS.md](PROCESS.md) — git 证据、外环站、内环 SOP
- [PHASES.md](PHASES.md) — 每一期做什么、改哪些 crate、对照哪些 reference 文件

CompanyOS 规范入口：

- [docs/company-os-overview.md](docs/company-os-overview.md) — 白话总览：心智模型、任务走读、术语词典和 FAQ（第一次读从这里开始）
- [docs/README.md](docs/README.md) — CompanyOS 文档总入口、权威层级、阅读路径和变更规则
- [docs/company-os-spec-index.md](docs/company-os-spec-index.md) — 规范阅读顺序、canonical owner、证据状态和实施阶段
- [docs/company-os-design.md](docs/company-os-design.md) — 总体产品、控制面、PMP、能力风险和安全边界
- [docs/company-os-domain-contracts.md](docs/company-os-domain-contracts.md) — Objective、Project、Milestone、Acceptance、Delivery、Outcome 等业务生命周期
- [docs/company-os-platform-architecture.md](docs/company-os-platform-architecture.md) — Runtime、Memory、Context/Cache、Capability/MCP、Workflow、Swarm、Provider 和 Observability
- [docs/company-os-operations-governance.md](docs/company-os-operations-governance.md) — Identity、Scheduler、Human Inbox、Artifact、Cost、Recovery、Data Governance 和 Connector
- [docs/company-os-quality-ecosystem.md](docs/company-os-quality-ecosystem.md) — Eval、Golden Trace、反馈学习、模型/Prompt 版本、代码知识和插件生态
- [docs/company-os-ui-ux.md](docs/company-os-ui-ux.md) — CLI/TTY、Web、Desktop、Run/Approval/Receipt/Recovery 的 UI 与交互规范
- [docs/company-os-implementation-outline.md](docs/company-os-implementation-outline.md) — 可认领的工程切片、验收条件和 90 天工程序列
- [docs/company-os-security-constitution.md](docs/company-os-security-constitution.md) — 安全宪法、负向路径和证明等级

v1.0 声明停在 `local_behavior`。真实可跑命令见 [USER.md](USER.md)。Claude Code / Desktop / 企业仍是北星，不是本版本完成。

```bash
# in a project folder (Codex/pi-style)
kiana trust .
kiana

# file manager / GUI target
kiana --workdir /path/to/project
kiana --pick-folder

# after the repo is trusted, one-shot still works

# v0.3 planning symposium (anti-meeting skips debate, still writes artifacts)
kiana run --symposium --anti-meeting --sandbox workspace-write --json -- \
  "create GOLDEN_PATH.txt containing hello"
# plan/DECISION.json + packet/TASK.json; Builder is not seated

# independent Builder consumes the packet
kiana run --packet packet/TASK.json --sandbox workspace-write --json

# v0.2 Builder direct-write still works
kiana run --sandbox workspace-write -- "create GOLDEN_PATH.txt containing hello"

# loopback Web workbench (dsh-shaped UI, same DaemonHost; SSE display stream, receipts authoritative)
kiana web --no-open --bind 127.0.0.1:3080

# installable Electron shell: opens kiana web, close can keep tray
bash scripts/install-desktop.sh
kiana-desktop

# Debian package (local, unsigned)
KIANA_BIN=target/debug/kiana bash scripts/package-desktop-deb.sh
sudo dpkg -i dist/kiana-desktop_0.1.0_amd64.deb
```

## 怎么接模型

模型服务商由 `KIANA_PROVIDER` 选择，默认是 `anthropic`。配置入口已经存在；本版本的
live provider 证明等级仍未完成，下面只说明怎么接线，能否跑通取决于你的本地服务、key
和网络环境。

| `KIANA_PROVIDER` | 怎么配 |
|---|---|
| `anthropic` | 必填 `ANTHROPIC_API_KEY`；可选 `ANTHROPIC_MODEL`、`ANTHROPIC_BASE_URL` |
| `openai-compatible` | 必填 `KIANA_OPENAI_API_KEY` 或 `OPENAI_API_KEY`；可选 `KIANA_OPENAI_MODEL`/`OPENAI_MODEL`、`KIANA_OPENAI_BASE_URL`/`OPENAI_BASE_URL` |
| `ollama` | 不需要 key；可选 `KIANA_OLLAMA_MODEL`/`OLLAMA_MODEL`、`KIANA_OLLAMA_BASE_URL`/`OLLAMA_BASE_URL`（默认 `http://localhost:11434`） |
| `fake` | 不需要 key；可选 `KIANA_FAKE_MODEL`，用于确定性本地测试 |

如果设置了 `KIANA_HARNESS_SCRIPT=/path/to/cassette.json`，它会优先于
`KIANA_PROVIDER`，运行本地 cassette，不会访问网络。

Ollama 本地例子：

```bash
ollama serve
ollama pull llama3.1

export KIANA_PROVIDER=ollama
export KIANA_OLLAMA_MODEL=llama3.1
# 可选；默认就是下面这个地址
export KIANA_OLLAMA_BASE_URL=http://localhost:11434

kiana trust .
kiana run --sandbox read-only -- "用一句话打个招呼"
```

Anthropic 或 OpenAI 兼容服务使用同一入口：

```bash
# Anthropic
export KIANA_PROVIDER=anthropic
export ANTHROPIC_API_KEY=your-key

# 或 OpenAI 兼容服务
export KIANA_PROVIDER=openai-compatible
export OPENAI_API_KEY=your-key
export OPENAI_BASE_URL=https://api.openai.com/v1
export OPENAI_MODEL=gpt-4.1
```

不要把 key 写进仓库、README、日志或截图。临时覆盖模型可以用
`kiana --model <name> -p <prompt>`。

CI / local cassette eval (not a live provider):

```bash
bash scripts/harness-golden-smoke.sh
bash scripts/v03-workbench-smoke.sh
bash scripts/v10-personal-lifecycle-smoke.sh
bash scripts/v10-p0-closeout-smoke.sh
bash scripts/v10-workbench-smoke.sh
```

Install demo (temp dir; does not claim `~/.local/bin` production-ready):

```bash
INSTALL_DIR=/tmp/kiana-bin KIANA_SKIP_PATH_SETUP=1 KIANA_SKIP_BUILD=1 \
  KIANA_BIN=target/debug/kiana bash install.sh
INSTALL_DIR=/tmp/kiana-bin bash install.sh --uninstall
```

License: MIT OR Apache-2.0.
