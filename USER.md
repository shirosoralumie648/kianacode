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

## 怎么接模型

模型服务商由 `KIANA_PROVIDER` 选择，默认是 `anthropic`。本版本对 live provider 的证明仍
停在配置和本地路径，能否跑通取决于你的本地服务、key 和网络环境。

| `KIANA_PROVIDER` | 怎么配 |
|---|---|
| `anthropic` | 必填 `ANTHROPIC_API_KEY`；可选 `ANTHROPIC_MODEL`、`ANTHROPIC_BASE_URL` |
| `openai-compatible` | 必填 `KIANA_OPENAI_API_KEY` 或 `OPENAI_API_KEY`；可选 `KIANA_OPENAI_MODEL`/`OPENAI_MODEL`、`KIANA_OPENAI_BASE_URL`/`OPENAI_BASE_URL` |
| `ollama` | 不需要 key；可选 `KIANA_OLLAMA_MODEL`/`OLLAMA_MODEL`、`KIANA_OLLAMA_BASE_URL`/`OLLAMA_BASE_URL`（默认 `http://localhost:11434`） |
| `fake` | 不需要 key；可选 `KIANA_FAKE_MODEL`，用于确定性本地测试 |

`KIANA_HARNESS_SCRIPT=/path/to/cassette.json` 的优先级最高，设置后会忽略 provider 选择并
运行本地 cassette，不会访问网络。

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

## 文件夹工作台（Codex / pi / dsh 那种入口）

产品路径是 `DaemonHost`，不是 parked 的 `kiana tui`。进目录就能干活；GUI 只负责选文件夹。
TTY 是对话区 + 输入框 + 状态行。`--json` 仍是脚本/cassette 路径。不声称 token 流式。

```bash
# 文件管理器：在项目目录打开终端，然后
kiana trust .
kiana
# TTY：conversation / input / status。输入需求后回车。
# Esc 或 Ctrl-C 取消正在跑的回合（同核 cancel_run）。
# /sandbox read-only 或 /sandbox workspace-write 会真切换；空 /sandbox 显示当前值。
# 默认 sandbox 是 workspace-write。TTY 未信任会先问；one-shot 失败码与 kiana run 相同：
# workspace_write_requires_trusted_non_safe_profile
# 不要会话面、只要 You>：KIANA_WORKBENCH_PLAIN=1 kiana

# 指定文件夹（.desktop 的 Exec 就是这个）
kiana --workdir /path/to/project -- "create GOLDEN_PATH.txt containing hello"

# GUI 选文件夹：zenity / kdialog / tkinter，或测试用 KIANA_FOLDER_PICKER_CMD
kiana --pick-folder
kiana gui
```

`contrib/kiana.desktop` 把「用 Kiana 打开文件夹」接到 `--workdir %f`。

闸门：`bash scripts/v10-workbench-smoke.sh`

## 网页工作台（dsh 形，同一核）

`kiana web` 是 DeepSeek Harness Web UI 的布局抄法：侧栏选会话、中间对话、右侧细节。工人仍是 `DaemonHost` / `KianaHarness`。只绑回环。不声称 token 流式，不是 dsh Cordis 克隆。

```bash
kiana trust .
kiana web --no-open --bind 127.0.0.1:3080
# 浏览器打开打印的 URL。Trust / sandbox / New session / Send / Cancel 都打同一核。
```

闸门：`cargo test -p kiana-entrypoints --test cli_web`

## 可安装桌面壳（Electron）

点开先到 Codex 形欢迎页：**打开文件夹**、**继续上次**、或 **先不选项目**。不选时自动在 `~/.kiana/workspaces/project-…` 建 scratch，工人只在那里写盘。关窗口可选 **Keep in background** 留托盘。只绑回环。不是签名商店包。

```bash
# 先有 kiana 二进制
INSTALL_DIR=/tmp/kiana-bin KIANA_SKIP_PATH_SETUP=1 KIANA_SKIP_BUILD=1 \
  KIANA_BIN=target/debug/kiana bash install.sh

# 再装桌面壳（需要 Node/npm；会下载 Electron）
INSTALL_DIR=/tmp/kiana-bin KIANA_DESKTOP_HOME=/tmp/kiana-desktop \
  KIANA_DESKTOP_DIR=/tmp/kiana-applications \
  KIANA_BIN=/tmp/kiana-bin/kiana bash scripts/install-desktop.sh

/tmp/kiana-bin/kiana-desktop
```

闸门：`node --test contrib/desktop/tests/*.js`

### `.deb` 包

把 CLI + Electron 壳打成可 `dpkg -i` 的本地包（不是签名 Debian 源）。

```bash
# 需要 node/npm/dpkg-deb，以及一份 kiana 二进制
# Electron 运行时约 100MB；第一次会从镜像拉 Chromium，不是卡住。
ELECTRON_MIRROR=https://npmmirror.com/mirrors/electron/ \
  KIANA_BIN=target/debug/kiana bash scripts/package-desktop-deb.sh
sudo dpkg -i dist/kiana-desktop_0.1.0_amd64.deb
# 应用菜单打开 Kiana，或 kiana-desktop / kiana --help
# 卸：sudo dpkg -r kiana-desktop
```

## 受信仓库里干活

仓库先 `kiana trust .`。`kiana run` 默认 sandbox 只读；写盘必须 `--sandbox workspace-write`。工作台默认已经是 workspace-write。

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
- 文件夹工作台是 `kiana` / `--workdir` / `--pick-folder`。TTY 会话面是 `workbench_chat`，不是 `kiana tui`。`kiana tui` 保持 park：legacy SDK/stream，不是 DaemonHost。
- 状态行只报 idle/running。token 流式、权限弹窗、`@文件`、跨进程 resume 都不是完成。
- HTTP MCP 是 `mcp_transport_unsupported`。stdio MCP 才是产品路径。
- live Anthropic / OpenAI / Ollama 不是完成。缺 tools 的 fake profile 必须 `unsupported_tools`，禁止假成功。
- JointSymposium、招满 COMPANY.md 角色、Librarian、TeamCreate/SendMessage 仍冻结。
- Daily / Research pack 仍是草案，除非同一核上先有黄金路径。
- SDK / IDE / Desktop / Web / git worktree 后开，不挡个人核心路径。
