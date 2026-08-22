# Kiana

Local-first agent **company OS**: PMP process groups are departments, roles have independent context/prompts/ACL, departments may hold bounded symposiums, memory is layered RAG.

v0.4 Phase 2（Coding pack 公开行为矩阵草稿）已签字：见 [docs/coding-pack-matrix.md](docs/coding-pack-matrix.md)。v0.4 Phase 1（Reviewer≠作者）仍必须绿。v0.3 规划部/执行部/有界会 + cassette eval/install 仍必须绿。v0.2 黄金路径仍必须绿（`kiana run` / print → daemon → `KianaHarness`，默认工人是执行部 Builder）。`kiana tui` **park**：仍走 legacy SDK/stream，不是产品路径。下一站是 v0.4.2 MCP client 经 daemon，不是五部门。

从 0 到完整产品的方案（按 `reference/` 中 coding agent 的 git 顺序 + 公司编制）在：

- [COMPANY.md](COMPANY.md) — 五部门、角色、有界群聊、六层 RAG/ACL、两个 Archon 怎么用
- [DESIGN.md](DESIGN.md) — 代码真相、版本阶梯、冻结、映射
- [PROCESS.md](PROCESS.md) — git 证据、外环站、内环 SOP
- [PHASES.md](PHASES.md) — 每一期做什么、改哪些 crate、对照哪些 reference 文件

这不是 1.0 声明。Claude Code / Desktop / 企业是北星，必须按阶梯打开。证明上限本里程碑是 `local_behavior`。

```bash
# after the repo is trusted
kiana trust .

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
```

Install demo (temp dir; does not claim `~/.local/bin` production-ready):

```bash
INSTALL_DIR=/tmp/kiana-phase4-bin KIANA_SKIP_PATH_SETUP=1 KIANA_SKIP_BUILD=1 \
  KIANA_BIN=target/debug/kiana bash install.sh
```

License: MIT OR Apache-2.0.
