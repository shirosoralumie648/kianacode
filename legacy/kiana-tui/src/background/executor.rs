//! Task executor for running background tasks.

use super::handle::TaskHandle;
use super::task::{
    BackgroundTask, ProgressSender, ProgressUpdate, TaskId, TaskMetadata, TaskStatus,
};
use crate::cancellation::CancellationToken;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};

/// Information about a running task.
struct TaskInfo {
    metadata: Arc<RwLock<TaskMetadata>>,
    status: Arc<RwLock<TaskStatus>>,
    cancel_token: CancellationToken,
}

/// Executes background tasks.
pub struct TaskExecutor {
    tasks: Arc<RwLock<HashMap<TaskId, TaskInfo>>>,
}

impl TaskExecutor {
    /// Creates a new task executor.
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Spawns a background task and returns a handle to it.
    pub fn spawn<T: Send + 'static>(
        &self,
        mut task: impl BackgroundTask<Output = T> + 'static,
    ) -> TaskHandle<T> {
        let metadata = Arc::new(RwLock::new(TaskMetadata::new(task.name())));
        let status = Arc::new(RwLock::new(TaskStatus::Pending));
        let cancel_token = CancellationToken::new();

        let id = metadata.read().unwrap().id;

        // Create channels
        let (result_tx, result_rx) = oneshot::channel();
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<ProgressUpdate>();

        // Store task info
        let task_info = TaskInfo {
            metadata: metadata.clone(),
            status: status.clone(),
            cancel_token: cancel_token.clone(),
        };
        self.tasks.write().unwrap().insert(id, task_info);

        // Clone for the task
        let metadata_clone = metadata.clone();
        let status_clone = status.clone();
        let cancel_token_clone = cancel_token.clone();
        let tasks_clone = self.tasks.clone();

        // Spawn the task
        tokio::spawn(async move {
            // Update status to running
            {
                let mut meta = metadata_clone.write().unwrap();
                meta.started_at = Some(Instant::now());
            }
            *status_clone.write().unwrap() = TaskStatus::Running {
                progress: None,
                message: None,
            };

            // Spawn progress updater. Apply updates in a sync helper so the
            // std::sync::RwLock guard cannot be held across await.
            let status_for_progress = status_clone.clone();
            let progress_handle = tokio::spawn(async move {
                while let Some(update) = progress_rx.recv().await {
                    apply_progress_update(&status_for_progress, update);
                }
            });

            // Execute the task
            let progress_sender = ProgressSender::new(progress_tx);
            let result = task
                .execute(cancel_token_clone.clone(), progress_sender)
                .await;

            // Stop progress updater
            progress_handle.abort();

            // Update status based on result
            let duration = {
                let meta = metadata_clone.read().unwrap();
                meta.started_at.unwrap().elapsed()
            };

            let final_status = if cancel_token_clone.is_cancelled() {
                TaskStatus::Cancelled {
                    reason: cancel_token_clone.reason(),
                }
            } else {
                match &result {
                    Ok(_) => TaskStatus::Completed { duration },
                    Err(e) => TaskStatus::Failed {
                        error: e.to_string(),
                        duration,
                    },
                }
            };

            {
                let mut meta = metadata_clone.write().unwrap();
                meta.completed_at = Some(Instant::now());
            }
            *status_clone.write().unwrap() = final_status;

            // Send result
            let _ = result_tx.send(result);

            // Clean up task info
            tasks_clone.write().unwrap().remove(&id);
        });

        TaskHandle::new(id, metadata, status, result_rx, cancel_token)
    }

    /// Gets the status of a task by ID.
    pub fn get_status(&self, id: TaskId) -> Option<TaskStatus> {
        self.tasks
            .read()
            .unwrap()
            .get(&id)
            .map(|info| info.status.read().unwrap().clone())
    }

    /// Lists all active tasks.
    pub fn list_tasks(&self) -> Vec<(TaskId, TaskMetadata, TaskStatus)> {
        self.tasks
            .read()
            .unwrap()
            .iter()
            .map(|(id, info)| {
                (
                    *id,
                    info.metadata.read().unwrap().clone(),
                    info.status.read().unwrap().clone(),
                )
            })
            .collect()
    }

    /// Cancels a task by ID.
    pub fn cancel_task(&self, id: TaskId) -> anyhow::Result<()> {
        let tasks = self.tasks.read().unwrap();
        if let Some(info) = tasks.get(&id) {
            info.cancel_token.cancel();
            Ok(())
        } else {
            anyhow::bail!("Task not found")
        }
    }

    /// Gets the number of active tasks.
    pub fn active_count(&self) -> usize {
        self.tasks.read().unwrap().len()
    }
}

impl Default for TaskExecutor {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_progress_update(status: &Arc<RwLock<TaskStatus>>, update: ProgressUpdate) {
    let mut status = status.write().unwrap();
    if let TaskStatus::Running { progress, message } = &mut *status {
        if let Some(p) = update.progress {
            *progress = Some(p);
        }
        if let Some(m) = update.message {
            *message = Some(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background::task::FunctionTask;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_executor_basic() {
        let executor = TaskExecutor::new();

        let task = FunctionTask::new("test", |_token, progress| async move {
            progress.message("starting");
            sleep(Duration::from_millis(10)).await;
            progress.set_progress(0.5);
            sleep(Duration::from_millis(10)).await;
            progress.message("done");
            Ok(42)
        });

        let handle = executor.spawn(task);
        let _id = handle.id();

        // Task should be active
        assert_eq!(executor.active_count(), 1);

        // Wait for completion
        let result = handle.await_result().await;
        assert_eq!(result.unwrap(), 42);

        // Task should be cleaned up
        sleep(Duration::from_millis(10)).await;
        assert_eq!(executor.active_count(), 0);
    }

    #[tokio::test]
    async fn test_executor_cancel() {
        let executor = TaskExecutor::new();

        let task = FunctionTask::new("test", |token, _progress| async move {
            loop {
                token.throw_if_cancelled()?;
                sleep(Duration::from_millis(10)).await;
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        });

        let handle = executor.spawn(task);
        let _id = handle.id();

        sleep(Duration::from_millis(20)).await;
        handle.cancel().unwrap();

        let result = handle.await_result().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_executor_list_tasks() {
        let executor = TaskExecutor::new();

        let task1 = FunctionTask::new("task1", |_token, _progress| async move {
            sleep(Duration::from_millis(100)).await;
            Ok(1)
        });

        let task2 = FunctionTask::new("task2", |_token, _progress| async move {
            sleep(Duration::from_millis(100)).await;
            Ok(2)
        });

        let _handle1 = executor.spawn(task1);
        let _handle2 = executor.spawn(task2);

        sleep(Duration::from_millis(10)).await;

        let tasks = executor.list_tasks();
        assert_eq!(tasks.len(), 2);
    }

    #[tokio::test]
    async fn test_executor_progress() {
        let executor = TaskExecutor::new();

        let task = FunctionTask::new("test", |_token, progress| async move {
            progress.set_progress(0.0);
            sleep(Duration::from_millis(10)).await;
            progress.set_progress(0.5);
            sleep(Duration::from_millis(10)).await;
            progress.set_progress(1.0);
            Ok(())
        });

        let handle = executor.spawn(task);
        let id = handle.id();

        sleep(Duration::from_millis(15)).await;

        if let Some(status) = executor.get_status(id) {
            if let TaskStatus::Running { progress, .. } = status {
                assert!(progress.is_some());
            }
        }

        let _ = handle.await_result().await;
    }
}
