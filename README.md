# Kiana

Local-first agent **company OS**: PMP process groups are departments, roles have independent context/prompts/ACL, departments may hold bounded symposiums, memory is layered RAG.

当前执行里程碑仍是 **v0.2：一条可跑的本地黄金路径**（`kiana run` / print → daemon → `KianaHarness`，工人是执行部 Builder）。

从 0 到完整产品的方案（按 `reference/` 中 coding agent 的 git 顺序 + 公司编制）在：

- [COMPANY.md](COMPANY.md) — 五部门、角色、有界群聊、六层 RAG/ACL、两个 Archon 怎么用
- [DESIGN.md](DESIGN.md) — 代码真相、版本阶梯、冻结、映射
- [PROCESS.md](PROCESS.md) — git 证据、外环站、内环 SOP
- [PHASES.md](PHASES.md) — 每一期做什么、改哪些 crate、对照哪些 reference 文件

这不是 1.0 声明。Claude Code / Desktop / 企业是北星，必须按阶梯打开。

```bash
# after a real provider is configured and the repo is trusted
kiana trust .
kiana run --sandbox workspace-write -- "create GOLDEN_PATH.txt containing hello"
```

License: MIT OR Apache-2.0.
