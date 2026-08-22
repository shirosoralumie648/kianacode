# TUI

Kiana 有两层 TUI 代码，完成度不同。文档必须分开写。

## 产品入口：`kiana tui`

实现路径：

- `kiana-entrypoints/src/cli.rs` 识别 `tui` 子命令
- `kiana-entrypoints/src/tui.rs` 创建 runtime
- `kiana-screens` 负责屏幕、REPL、history、settings

需要交互式 stdin/stdout，不能从管道或后台进程启动。

```bash
cargo build -p kiana-entrypoints --bin kiana --locked
./target/debug/kiana tui
```

已接线的能力：

- 本地 slash command（走 `kiana-commands` registry，不把 `/help` 一类命令泄漏给模型）
- session resume / record-only reply / compact / fork
- `/history`：打开 prompt 历史选择器
- 历史文件：`KIANA_HOME/tui-history.jsonl`（去重、最新优先，限制见 `TUI_PROMPT_HISTORY_LIMIT`）
- 历史界面：`/` 搜索，`j`/`k` 或方向键导航，Enter 恢复为草稿，Esc 返回 REPL
- 历史搜索激活时 `Ctrl+R` 打开搜索历史（`kiana-screens/src/history.rs`）
- footer 提示：`/doctor` `/resume` `/history` `/settings`
- Ctrl+C 退出（需要确认的场景走 y/n）

这是当前用户应该使用的 TUI。

## 组件库：`kiana-tui`

`kiana-tui` crate 在 master 上已落地大量可复用模块，例如：

- overlay / fuzzy search / regex / multi-field search
- status bar、toast、command palette、help overlay、confirm dialog
- layout（panel / split / tab / virtual list）
- table、tree、charts、diff、folding、markdown、syntax
- background tasks、cancellation、queue、live update、macro、state machine

这些模块有单元测试和部分集成测试，**不等于**全部已经接到 `kiana tui` 主循环。

独立二进制：

```bash
cargo run -p kiana-tui --bin kiana-tui
```

该二进制演示了 SearchOverlay、status bar、toast 等组件，是组件试验床，不是产品默认入口。

## 不要这样写完成说明

| 错误说法 | 正确边界 |
| --- | --- |
| “TUI 已完成 50 个功能” | crate 模块存在 ≠ `kiana tui` 已接线 |
| “Ctrl+Q 退出” | 产品 TUI 用 Ctrl+C |
| “Ctrl+R 搜索全部对话，Ctrl+U 切换模式” | 这是 `kiana-tui` overlay 的行为；产品入口是 `/history` + 历史页 `Ctrl+R` |
| “plugins/scripting 已随 TUI 发布” | 这两个 crate 未进入 master workspace |

## 相关代码

| 路径 | 角色 |
| --- | --- |
| `kiana-entrypoints/src/tui.rs` | 产品 TUI runtime |
| `kiana-screens/src/app.rs` | 屏幕状态机 |
| `kiana-screens/src/repl.rs` | REPL 输入与 slash command |
| `kiana-screens/src/history.rs` | `/history` 与 Ctrl+R 搜索历史 |
| `kiana-tui/src/lib.rs` | 组件库导出 |
| `kiana-tui/src/main.rs` | 独立实验二进制 |
