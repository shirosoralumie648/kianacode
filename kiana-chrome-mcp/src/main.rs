#[tokio::main]
async fn main() -> anyhow::Result<()> {
    kiana_chrome_mcp::run_stdio().await
}
