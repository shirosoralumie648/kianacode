# P2-M7-01 无障碍回退基线

> 快照日期：2026-09-18。本文记录 Web/TTY 的键盘、窄屏、文本状态、ARIA/高对比回退；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Web fallback

Web 页面为所有主要区域、状态和输入提供 `aria-label`/`aria-live`/`aria-describedby`、region/status/dialog roles、skip link、focus-visible 和可聚焦文本区域；审批、错误、stream gap/incomplete、cancelled/result_unknown、receipt 和 read-only 状态通过 textContent/announcer 明确展示，不依赖颜色或 delta 猜测完成。

CSS 在窄屏（`max-width:650px`）下改为纵向布局并限制聊天区高度；`prefers-contrast:more`、`forced-colors:active` 提高边框/文本可辨识度，`prefers-reduced-motion:reduce` 关闭动画/平滑滚动。modal coach 记录前一焦点并在关闭后恢复，动态 session 按 `aria-current` 标记。

## TTY/text fallback

Workbench/TTY 保留键盘驱动的 Enter/Ctrl-Enter 提交、Esc/Ctrl-C 取消、Tab/箭头导航和纯文本状态栏；无法显示 stream 时明确提示 incomplete/receipt authoritative。旧 rustyline workbench 的 approval prompt 仍是文本确认，实际 decision 继续由 server challenge/ControlPlane 校验。

Desktop 复用 Web shell，不另建渲染或执行循环；无颜色、窄终端或屏幕阅读器环境至少可读出状态、错误、待审批动作和回执引用。

## CI-only 验收

`status_is_reachable_without_color` source guard 覆盖 ARIA/status/focus/keyboard/narrow/high-contrast/reduced-motion/text fallback 和 Desktop reuse；现有 Web page status/focus tests 与 Workbench chat tests 作为回归。

```text
cargo fmt --all --check
cargo test -p kiana-entrypoints --test p2_m7_01_accessibility --locked -- --test-threads=1
cargo test -p kiana-entrypoints --lib web_page_ --locked -- --test-threads=1
cargo test -p kiana-entrypoints --lib workbench_chat::tests --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- 当前证明覆盖源码与现有 Web/TTY unit/loopback fixtures；没有浏览器/屏幕阅读器自动化、真实键盘/窄屏设备矩阵、OS high-contrast/live desktop packaging 或 external human auth proof。
- ARIA/text/status 只是投影，不能替代 EventLog/Receipt/ControlPlane；颜色、focus 和 stream delta 丢失时均回退到可读文本和 receipt/refresh，不宣称执行成功。
- Desktop 仅复用 Web asset；原生托盘/通知/系统辅助技术和跨进程 durable UI state 留 UI-26+、NM/DEP/SC。
