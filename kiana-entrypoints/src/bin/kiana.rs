//! `kiana` 可执行文件的最薄入口。
//!
//! 初始化和清理由 entrypoint 共用模块集中管理，实际命令路由仍进入既有 `cli::main`。
//! 此处不复制权限判断或模型执行逻辑，避免二进制入口形成独立执行路径。

use anyhow::Result;
use kiana_entrypoints::{cli, init};

/// 初始化进程级资源，运行 CLI，并尽力执行收尾处理。
///
/// CLI 返回值保持为主结果；清理失败只写入标准错误，不能覆盖已经产生的命令成功或失败。
/// 这保证调用者仍能按命令语义处理退出状态，同时保留清理异常的可观察性。
#[tokio::main]
async fn main() -> Result<()> {
    init::init().await?;
    let result = cli::main().await;
    if let Err(error) = init::run_cleanup_handlers().await {
        eprintln!("session cleanup error: {error}");
    }
    result
}
