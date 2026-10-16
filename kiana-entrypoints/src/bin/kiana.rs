//! `kiana` 可执行文件的最薄入口。
//!
//! 初始化和清理由 entrypoint 共用模块集中管理，实际命令路由仍进入既有 `cli::main`。
//! 此处不复制权限判断或模型执行逻辑，避免二进制入口形成独立执行路径。

use anyhow::Result;
use kiana_entrypoints::{cli, init};

/// 初始化进程级资源，运行 CLI，并尽力执行收尾处理。
///
/// 【入口与调用关系】操作系统启动 `kiana` 二进制后进入此函数；`#[tokio::main]` 宏会创建 Tokio
/// async runtime，使这里能用 `.await` 调用初始化、CLI 和清理流程。CLI 自己负责读取命令行参数并路由
/// 到 `DaemonHost`，此薄入口不解析业务命令，也不另外启动模型/工具执行循环。
/// 【核心顺序】先 `init::init()` 准备进程环境与退出清理器，再把实际工作交给 `cli::main()`；无论 CLI
/// 返回 `Ok` 还是 `Err`，都先尝试运行已注册的 cleanup handlers。
/// 【输出】CLI 的 `anyhow::Result` 原样作为二进制的主结果返回；收尾处理错误仅打印到标准错误，不覆盖
/// CLI 已产生的成功或失败结果。清理成功也不证明外部服务一定完成资源回收。
/// 【失败边界】如果初始化返回 `Err`，`?` 会直接返回，CLI 和下方清理步骤都不会执行；当前 `init()`
/// 对可恢复初始化异常主要记录状态/日志并返回成功。若 CLI panic 或进程被强制终止，也不会走到常规清理。
/// ⚠ 不要把 cleanup 错误改成覆盖 `result`：那会让命令本身的退出结果被次级收尾错误改写。
#[tokio::main]
async fn main() -> Result<()> {
    init::init().await?;
    let result = cli::main().await;
    if let Err(error) = init::run_cleanup_handlers().await {
        eprintln!("session cleanup error: {error}");
    }
    result
}
