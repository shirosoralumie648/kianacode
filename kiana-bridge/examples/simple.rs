use kiana_bridge::{
    BridgeApiClient, BridgeConfig, CommandBridgeSessionRunner, SessionManager, SpawnMode,
    WorkPollLoop,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = BridgeConfig {
        dir: std::env::current_dir()?.to_string_lossy().to_string(),
        machine_name: local_machine_name(),
        branch: "main".to_string(),
        git_repo_url: None,
        max_sessions: 1,
        spawn_mode: SpawnMode::SingleSession,
        bridge_id: uuid::Uuid::new_v4().to_string(),
        worker_type: "claude_code".to_string(),
        environment_id: uuid::Uuid::new_v4().to_string(),
        api_base_url: "https://api.anthropic.com".to_string(),
        session_ingress_url: "https://api.anthropic.com".to_string(),
        heartbeat_interval_ms: 60_000,
        session_timeout_ms: 24 * 60 * 60 * 1000,
        ccr_v2_sse_reconnect_give_up_ms: None,
        ccr_v2_sse_liveness_timeout_ms: None,
        debug_file: None,
        permission_mode: None,
    };

    let access_token = std::env::var("CLAUDE_ACCESS_TOKEN")
        .expect("CLAUDE_ACCESS_TOKEN environment variable not set");

    let api = Arc::new(BridgeApiClient::new(
        config.api_base_url.clone(),
        access_token,
    ));

    println!("Registering bridge environment...");
    let (environment_id, environment_secret) = api.register_environment(&config).await?;
    println!("Environment registered: {}", environment_id);

    let session_manager = Arc::new(SessionManager::new());

    let runner = CommandBridgeSessionRunner::for_config(&config)?;
    let runner_command = runner.command().to_string();
    let runner_args = runner.args().to_vec();
    let work_loop = WorkPollLoop::with_runner(
        api.clone(),
        session_manager.clone(),
        config,
        Arc::new(runner),
    );

    println!(
        "Starting work poll loop with command runner: {} {}",
        runner_command,
        runner_args.join(" ")
    );
    work_loop.run(environment_id, environment_secret).await?;

    Ok(())
}

fn local_machine_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
