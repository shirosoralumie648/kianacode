use anyhow::Result;
use kiana_entrypoints::{cli, init};

#[tokio::main]
async fn main() -> Result<()> {
    init::init().await?;
    let result = cli::main().await;
    if let Err(error) = init::run_cleanup_handlers().await {
        eprintln!("session cleanup error: {error}");
    }
    result
}
