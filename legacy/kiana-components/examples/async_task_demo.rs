//! Async Task Manager Demo
//!
//! Demonstrates the capabilities of the async task manager:
//! 1. Simple task execution
//! 2. Tasks with progress reporting
//! 3. Cancellable tasks
//! 4. Multiple concurrent tasks
//! 5. Task monitoring (like k9s)

use kiana_components::async_task::{AsyncTaskManager, TaskError, TaskPriority};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Async Task Manager Demo ===\n");

    demo_1_simple_task().await?;
    demo_2_progress_reporting().await?;
    demo_3_cancellable_task().await?;
    demo_4_concurrent_tasks().await?;
    demo_5_k9s_style_monitoring().await?;

    println!("\n=== All demos completed successfully! ===");
    Ok(())
}

/// Demo 1: Simple task execution
async fn demo_1_simple_task() -> Result<(), TaskError> {
    println!("📋 Demo 1: Simple Task Execution");
    println!("Creating a task that fetches data...\n");

    let manager = AsyncTaskManager::new();

    let handle = manager.spawn("fetch-data", async {
        println!("  → Task started: fetching data...");
        tokio::time::sleep(Duration::from_millis(500)).await;
        println!("  → Task completed: data retrieved");
        Ok::<_, TaskError>("sample data".to_string())
    });

    println!("  Task ID: {}", handle.id());
    println!("  Waiting for completion...\n");

    let result = handle.wait().await?;
    println!("  ✓ Result: {}\n", result);

    Ok(())
}

/// Demo 2: Progress reporting
async fn demo_2_progress_reporting() -> Result<(), TaskError> {
    println!("📊 Demo 2: Progress Reporting");
    println!("Processing files with progress updates...\n");

    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_context("process-files", |ctx| async move {
        let files = vec![
            "file1.txt",
            "file2.txt",
            "file3.txt",
            "file4.txt",
            "file5.txt",
        ];

        for (i, file) in files.iter().enumerate() {
            ctx.check_cancellation().await?;

            ctx.report_progress(
                i as u64,
                files.len() as u64,
                Some(format!("Processing {}", file)),
            )
            .await?;

            println!("  → Processing {} ({}/{})", file, i + 1, files.len());
            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        ctx.report_complete(Some("All files processed".to_string()))
            .await?;

        Ok::<_, TaskError>(files.len())
    });

    let count = handle.wait().await?;
    println!("  ✓ Processed {} files\n", count);

    Ok(())
}

/// Demo 3: Cancellable task
async fn demo_3_cancellable_task() -> Result<(), TaskError> {
    println!("🛑 Demo 3: Cancellable Task");
    println!("Starting a long-running task and cancelling it...\n");

    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_context("long-operation", |ctx| async move {
        for i in 0..100 {
            if ctx.should_cancel() {
                println!("  → Task received cancellation signal at iteration {}", i);
                return Err(TaskError::cancelled());
            }

            if i % 10 == 0 {
                println!("  → Progress: {} iterations", i);
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        Ok::<_, TaskError>(())
    });

    // Let it run for a bit
    tokio::time::sleep(Duration::from_millis(250)).await;

    println!("  → Sending cancellation signal...");
    handle.cancel().await;

    match handle.wait().await {
        Ok(_) => println!("  ✗ Task should have been cancelled"),
        Err(e) if e.is_cancelled() => println!("  ✓ Task cancelled successfully\n"),
        Err(e) => println!("  ✗ Unexpected error: {}\n", e),
    }

    Ok(())
}

/// Demo 4: Multiple concurrent tasks
async fn demo_4_concurrent_tasks() -> Result<(), TaskError> {
    println!("🔄 Demo 4: Multiple Concurrent Tasks");
    println!("Running 3 tasks with different priorities...\n");

    let manager = AsyncTaskManager::new();

    let handle1 = manager.spawn_with_priority("low-priority", TaskPriority::Low, async {
        println!("  → Low priority task started");
        tokio::time::sleep(Duration::from_millis(300)).await;
        println!("  → Low priority task completed");
        Ok::<_, TaskError>(1)
    });

    let handle2 = manager.spawn_with_priority("normal-priority", TaskPriority::Normal, async {
        println!("  → Normal priority task started");
        tokio::time::sleep(Duration::from_millis(200)).await;
        println!("  → Normal priority task completed");
        Ok::<_, TaskError>(2)
    });

    let handle3 = manager.spawn_with_priority("high-priority", TaskPriority::High, async {
        println!("  → High priority task started");
        tokio::time::sleep(Duration::from_millis(100)).await;
        println!("  → High priority task completed");
        Ok::<_, TaskError>(3)
    });

    // Wait for all tasks
    let results = tokio::try_join!(handle1.wait(), handle2.wait(), handle3.wait(),)?;

    println!("  ✓ All tasks completed: {:?}\n", results);

    Ok(())
}

/// Demo 5: k9s-style continuous monitoring
async fn demo_5_k9s_style_monitoring() -> Result<(), TaskError> {
    println!("🔍 Demo 5: k9s-Style Continuous Monitoring");
    println!("Monitoring resources with periodic updates...\n");

    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_context("watch-resources", |ctx| async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut count = 0;

        loop {
            tokio::select! {
                _ = ctx.cancelled() => {
                    println!("  → Monitoring stopped");
                    ctx.report_complete(Some(format!("Collected {} snapshots", count))).await?;
                    return Ok::<_, TaskError>(count);
                }
                _ = interval.tick() => {
                    count += 1;
                    let pods = simulate_fetch_pods();

                    ctx.report_indeterminate(Some(format!("Snapshot {}: {} pods", count, pods.len())))
                        .await?;

                    println!("  → Snapshot {}: Found {} pods", count, pods.len());

                    if count >= 5 {
                        println!("  → Collected enough snapshots");
                        return Ok(count);
                    }
                }
            }
        }
    });

    let snapshots = handle.wait().await?;
    println!(
        "  ✓ Monitoring complete: {} snapshots collected\n",
        snapshots
    );

    Ok(())
}

/// Simulate fetching Kubernetes pods (like k9s does)
fn simulate_fetch_pods() -> Vec<String> {
    let count = 3
        + (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            % 5);

    (0..count).map(|i| format!("pod-{}", i)).collect()
}
