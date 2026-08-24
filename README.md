# Kiana

Local-first agent **company OS**: PMP process groups are departments, roles have independent context/prompts/ACL, departments may hold bounded symposiums, memory is layered RAG.

v1.0 已在 `local_behavior` 上限下勾选：temp `INSTALL_DIR` 生命周期 + [USER.md](USER.md) + P0 审计 + [NOTICE](NOTICE)。v1.0.4 工作台会话面（TTY conversation / input / status，同一 `DaemonHost`）已绿；`kiana tui` 仍 park。不声称 token 流式。这不是 live provider、不是 `~/.local/bin` 生产安装、不是签名包、不是企业、不是 dsh Web UI 克隆。v0.6.1 ORCH-04（同核两个 packet Builder + 路径锁；越权 `packet_path_denied`）仍必须绿。v0.5 later SYMP-04（五部门有界会；规划会回归不破；联席冻结）仍必须绿。v0.5 later LONG-02（超预算 compact 进收据；同核 continue 之后仍能写盘；pause 仍是 cancel）仍必须绿。v0.5.2 六层记忆 ACL 与 v0.5.1 五个部门对象仍必须绿。v0.4 Coding pack 仍必须绿（P1-READ skipped：搜索走 `shell`）。v0.3 规划部/执行部/有界会 + cassette eval/install 仍必须绿。v0.2 黄金路径仍必须绿（`kiana run` / print → daemon → `KianaHarness`，默认工人是执行部 Builder）。`kiana tui` **park**：仍走 legacy SDK/stream，不是产品路径。Daily/Research 仍草案。不要打开 v1.x / worktree / SDK/IDE / JointSymposium。

从 0 到完整产品的方案（按 `reference/` 中 coding agent 的 git 顺序 + 公司编制）在：

- [COMPANY.md](COMPANY.md) — 五部门、角色、有界群聊、六层 RAG/ACL、两个 Archon 怎么用
- [DESIGN.md](DESIGN.md) — 代码真相、版本阶梯、冻结、映射
- [PROCESS.md](PROCESS.md) — git 证据、外环站、内环 SOP
- [PHASES.md](PHASES.md) — 每一期做什么、改哪些 crate、对照哪些 reference 文件

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
```

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
