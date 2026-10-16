//! 独立 `kiana-chrome-mcp` 二进制入口。
//!
//! Cargo 启动本程序后，Tokio runtime 调用 crate 的 `run_stdio()`，由其读取 MCP JSON-RPC、
//! 按需启动 Chromium 并将响应写回 stdout。Kiana 主 CLI 的 `--claude-in-chrome-mcp` 分支
//! 也会进入同一函数；本文件没有创建 DaemonHost 或另一个 Harness。

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    kiana_chrome_mcp::run_stdio().await
}
