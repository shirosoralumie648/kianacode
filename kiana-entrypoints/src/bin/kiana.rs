//! `kiana` 可执行文件的最薄入口。
//!
//! 初始化和清理由 entrypoint 共用模块集中管理，实际命令路由仍进入既有 `cli::main`。
//! 此处不复制权限判断或模型执行逻辑，避免二进制入口形成独立执行路径。

use anyhow::Result;
use kiana_entrypoints::{cli, init};

/// 命令主线程的栈大小：16 MiB。
///
/// 【为什么需要显式指定】Windows 上 `#[tokio::main]` 展开出的运行时代码会把整个 CLI 分派、
/// 命令执行和（在 `run` 路径上）DaemonHost 组合根都跑在进程主线程上，而 Windows 默认主线程
/// 栈只有 1 MiB。调试构建下 `kiana_bootstrap::config::load_config`、命令注册表等深层调用一旦
/// 叠加就触发 `has overflowed its stack`，表现为几乎所有本地命令直接 exit 127。栈大小属于
/// 进程入口的职责，不能交给下游模块「少用一点栈」来解决。
const MAIN_THREAD_STACK_BYTES: usize = 16 * 1024 * 1024;

/// 运行 CLI 并把结果原样返回；仅由 `main` 在专用线程上调用。
fn run() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        init::init().await?;
        cli::main().await
    });
    if let Err(error) = runtime.block_on(init::run_cleanup_handlers()) {
        eprintln!("session cleanup error: {error}");
    }
    result
}

/// 初始化进程级资源，运行 CLI，并尽力执行收尾处理。
///
/// 【入口与调用关系】操作系统启动 `kiana` 二进制后进入此函数；它在一个显式指定栈大小的
/// 线程上建立 Tokio runtime（[`run`]），使 CLI 能用 `.await` 调用初始化、命令路由和清理流程。
/// CLI 自己负责读取命令行参数并路由到 `DaemonHost`，此薄入口不解析业务命令，也不另外启动
/// 模型/工具执行循环。
/// 【核心顺序】先 `init::init()` 准备进程环境与退出清理器，再把实际工作交给 `cli::main()`；无论 CLI
/// 返回 `Ok` 还是 `Err`，都先尝试运行已注册的 cleanup handlers。
/// 【输出】CLI 的 `anyhow::Result` 原样作为二进制的主结果返回；收尾处理错误仅打印到标准错误，不覆盖
/// CLI 已产生的成功或失败结果。清理成功也不证明外部服务一定完成资源回收。
/// 【失败边界】如果初始化返回 `Err`，`?` 会直接返回，CLI 和下方清理步骤都不会执行；当前 `init()`
/// 对可恢复初始化异常主要记录状态/日志并返回成功。若 CLI panic 或进程被强制终止，也不会走到常规清理。
/// ⚠ 不要把 cleanup 错误改成覆盖 `result`：那会让命令本身的退出结果被次级收尾错误改写。
fn main() -> Result<()> {
    // 主线程只等子线程：`?`/`resume_unwind` 之后返回的 `Result` 与原先
    // `#[tokio::main] -> Result<()>` 完全一致（Ok -> exit 0，Err -> stderr 打印并 exit 1）。
    match std::thread::Builder::new()
        .name("kiana-main".to_owned())
        .stack_size(MAIN_THREAD_STACK_BYTES)
        .spawn(run)?
        .join()
    {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
