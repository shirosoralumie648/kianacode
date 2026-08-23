# Kiana 用户手册

这是现在真能跑的命令，不是 104 条需求表。

证明上限是 `local_behavior`：受信仓库 + cassette / fake-script。live provider、`~/.local/bin` 生产安装、签名包都不是完成。

## 安装 / 升级 / 回滚 / 恢复 / 卸载

默认 `INSTALL_DIR` 是 `~/.local/bin`。下面用临时目录做生命周期；不要把写入家目录当成完成。

```bash
# 需要一份已有二进制。优先 target/debug/kiana，不要用过期的 target/release/kiana。
INSTALL_DIR=/tmp/kiana-bin KIANA_SKIP_PATH_SETUP=1 KIANA_SKIP_BUILD=1 \
  KIANA_BIN=target/debug/kiana bash install.sh
/tmp/kiana-bin/kiana --version

# 升级：再装同一份二进制，checksum 应不变
INSTALL_DIR=/tmp/kiana-bin KIANA_SKIP_PATH_SETUP=1 KIANA_SKIP_BUILD=1 \
  KIANA_BIN=target/debug/kiana bash install.sh

# 回滚：把安装时备份的二进制拷回去
# cp "$BACKUP/kiana" /tmp/kiana-bin/kiana && chmod +x /tmp/kiana-bin/kiana

# 恢复：删掉再装
rm -f /tmp/kiana-bin/kiana
INSTALL_DIR=/tmp/kiana-bin KIANA_SKIP_PATH_SETUP=1 KIANA_SKIP_BUILD=1 \
  KIANA_BIN=target/debug/kiana bash install.sh

# 卸载（幂等）
INSTALL_DIR=/tmp/kiana-bin bash install.sh --uninstall
INSTALL_DIR=/tmp/kiana-bin bash install.sh --uninstall
```

闸门：`bash scripts/v10-personal-lifecycle-smoke.sh`

## 受信仓库里干活

仓库先 `kiana trust .`。默认 sandbox 只读；写盘必须 `--sandbox workspace-write`。

```bash
kiana trust .

# 执行部 Builder 直接写盘（v0.2 黄金路径）
kiana run --sandbox workspace-write -- "create a file named GOLDEN_PATH.txt containing hello"

# 规划会。anti-meeting 跳过辩论，仍写 plan/DECISION.json + packet/TASK.json；Builder 不列席
kiana run --symposium --anti-meeting --sandbox workspace-write --json -- \
  "create GOLDEN_PATH.txt containing hello"

# 独立 Builder 消费 packet（新 session，不复制编排器 transcript）
kiana run --packet packet/TASK.json --sandbox workspace-write --json

# 监控部 Reviewer ≠ 作者。确定性门，不跑模型；写出 gate/REVIEW.json
kiana run --review <author_session_id>
```

cassette 回归：

```bash
bash scripts/harness-golden-smoke.sh
bash scripts/v03-workbench-smoke.sh
```

## 诚实边界

- 并行 Builder 是同一 `DaemonHost` 上的 `spawn`，**没有**新 CLI。路径锁在 ControlPlane；越权是 `packet_path_denied`。
- `kiana tui` 保持 park：legacy SDK/stream，不是 DaemonHost。
- HTTP MCP 是 `mcp_transport_unsupported`。stdio MCP 才是产品路径。
- live Anthropic / OpenAI / Ollama 不是完成。缺 tools 的 fake profile 必须 `unsupported_tools`，禁止假成功。
- JointSymposium、招满 COMPANY.md 角色、Librarian、TeamCreate/SendMessage 仍冻结。
- Daily / Research pack 仍是草案，除非同一核上先有黄金路径。
- SDK / IDE / Desktop / Web / git worktree 后开，不挡个人核心路径。
