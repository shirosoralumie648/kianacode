//! Integration tests for async task manager

use kiana_components::async_task::{
    AsyncTaskManager, TaskError, TaskHandle, TaskPriority, TaskStatus,
};
use std::time::Duration;

#[tokio::test]
async fn test_basic_task_execution() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn("basic-task", async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok::<_, TaskError>(42)
    });

    assert_eq!(handle.id().as_u64(), 1);

    let result = handle.wait().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 42);
}

#[tokio::test]
async fn test_task_with_progress() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_context("progress-task", |ctx| async move {
        ctx.report_progress(0, 100, Some("Starting".to_string()))
            .await?;
        tokio::time::sleep(Duration::from_millis(10)).await;

        ctx.report_progress(50, 100, Some("Halfway".to_string()))
            .await?;
        tokio::time::sleep(Duration::from_millis(10)).await;

        ctx.report_complete(Some("Done".to_string())).await?;
        Ok::<_, TaskError>(())
    });

    let result = handle.wait().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_task_cancellation() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_context("long-task", |ctx| async move {
        for i in 0..1000 {
            ctx.check_cancellation().await?;
            tokio::time::sleep(Duration::from_millis(5)).await;
            ctx.report_progress(i, 1000, None).await?;
        }
        Ok::<_, TaskError>(())
    });

    // Let it run a bit
    tokio::time::sleep(Duration::from_millis(30)).await;

    // Cancel it
    handle.cancel().await;
    assert!(handle.is_cancelled());

    let result = handle.wait().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn test_multiple_tasks() {
    let manager = AsyncTaskManager::new();

    let handle1 = manager.spawn("task-1", async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok::<_, TaskError>(1)
    });

    let handle2 = manager.spawn("task-2", async {
        tokio::time::sleep(Duration::from_millis(15)).await;
        Ok::<_, TaskError>(2)
    });

    let handle3 = manager.spawn("task-3", async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        Ok::<_, TaskError>(3)
    });

    // Wait for all
    let result1 = handle1.wait().await.unwrap();
    let result2 = handle2.wait().await.unwrap();
    let result3 = handle3.wait().await.unwrap();

    assert_eq!(result1, 1);
    assert_eq!(result2, 2);
    assert_eq!(result3, 3);
}

#[tokio::test]
async fn test_task_priority() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_priority("high-priority", TaskPriority::High, async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok::<_, TaskError>(())
    });

    tokio::time::sleep(Duration::from_millis(5)).await;

    let tasks = manager.list_tasks().await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "high-priority");
    assert_eq!(tasks[0].priority, TaskPriority::High);

    handle.wait().await.unwrap();
}

#[tokio::test]
async fn test_task_error_handling() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn("failing-task", async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Err::<(), _>(TaskError::custom("intentional failure"))
    });

    let result = handle.wait().await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.message, "intentional failure");
}

#[tokio::test]
async fn test_indeterminate_progress() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_context("indeterminate-task", |ctx| async move {
        ctx.report_indeterminate(Some("Processing...".to_string()))
            .await?;
        tokio::time::sleep(Duration::from_millis(20)).await;

        ctx.report_indeterminate(Some("Still processing...".to_string()))
            .await?;
        tokio::time::sleep(Duration::from_millis(20)).await;

        ctx.report_complete(Some("Finished".to_string())).await?;
        Ok::<_, TaskError>(())
    });

    let result = handle.wait().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_task_status_tracking() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn("status-task", async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok::<_, TaskError>(())
    });

    let id = handle.id();

    // Check status while running
    tokio::time::sleep(Duration::from_millis(5)).await;
    let status = manager.get_task_status(id).await;
    assert!(status.is_some());
    assert_eq!(status.unwrap(), TaskStatus::Running);

    // Wait for completion
    handle.wait().await.unwrap();

    // Task should still be tracked
    let metadata = manager.get_task(id).await;
    assert!(metadata.is_some());
}

#[tokio::test]
async fn test_cleanup_completed_tasks() {
    let manager = AsyncTaskManager::new();

    let handle1 = manager.spawn("task-1", async { Ok::<_, TaskError>(()) });

    let handle2 = manager.spawn("task-2", async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok::<_, TaskError>(())
    });

    // Wait for first task
    handle1.wait().await.unwrap();

    // Should have 2 tasks
    assert_eq!(manager.total_count().await, 2);

    // Cleanup completed
    manager.cleanup_completed().await;

    // Should have 1 task remaining (the long-running one)
    assert_eq!(manager.total_count().await, 1);

    // Cancel the remaining task
    handle2.cancel().await;
    handle2.wait().await.ok();
}

#[tokio::test]
async fn test_cancel_all_tasks() {
    let manager = AsyncTaskManager::new();

    let handle1: TaskHandle<()> = manager.spawn_with_context("task-1", |ctx| async move {
        loop {
            ctx.check_cancellation().await?;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    let handle2: TaskHandle<()> = manager.spawn_with_context("task-2", |ctx| async move {
        loop {
            ctx.check_cancellation().await?;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Cancel all
    manager.cancel_all().await;

    // Both should be cancelled
    let result1 = handle1.wait().await;
    let result2 = handle2.wait().await;

    assert!(result1.is_err());
    assert!(result2.is_err());
    assert!(result1.unwrap_err().is_cancelled());
    assert!(result2.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn test_progress_polling() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_context("polling-task", |ctx| async move {
        for i in 0..5 {
            ctx.report_progress(i, 5, Some(format!("Step {}", i)))
                .await?;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok::<_, TaskError>(())
    });

    // Poll for progress updates
    let mut updates = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(progress) = handle.poll_progress().await {
            updates.push(progress.current);
        }

        if handle.is_complete().await {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }

        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    handle.wait().await.unwrap();

    // Should have received some progress updates
    assert!(!updates.is_empty());
}

#[tokio::test]
async fn test_task_metadata() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_priority("metadata-task", TaskPriority::Critical, async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok::<_, TaskError>(())
    });

    let id = handle.id();
    tokio::time::sleep(Duration::from_millis(5)).await;

    let metadata = manager.get_task(id).await.unwrap();
    assert_eq!(metadata.name, "metadata-task");
    assert_eq!(metadata.priority, TaskPriority::Critical);
    assert!(metadata.started_at.is_some());
    assert!(metadata.completed_at.is_none());

    handle.wait().await.unwrap();
}

#[tokio::test]
async fn test_concurrent_progress_updates() {
    let manager = AsyncTaskManager::new();

    let handle = manager.spawn_with_context("concurrent-progress", |ctx| async move {
        // Rapidly send progress updates
        for i in 0..100 {
            ctx.report_progress(i, 100, None).await?;
        }
        Ok::<_, TaskError>(())
    });

    let result = handle.wait().await;
    assert!(result.is_ok());
}
