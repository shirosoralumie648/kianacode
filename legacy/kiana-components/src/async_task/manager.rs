//! Async task manager core implementation

use crate::async_task::{
    CancellationToken, TaskContext, TaskHandle, TaskId, TaskMetadata, TaskPriority, TaskProgress,
    TaskResult, TaskStatus,
};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, RwLock};

/// Internal task information
struct TaskInfo {
    metadata: TaskMetadata,
    status: Arc<RwLock<TaskStatus>>,
    cancel_token: CancellationToken,
}

/// Async task manager for spawning and managing background tasks
pub struct AsyncTaskManager {
    tasks: Arc<Mutex<HashMap<TaskId, TaskInfo>>>,
    next_id: Arc<AtomicU64>,
    runtime_handle: tokio::runtime::Handle,
}

impl AsyncTaskManager {
    /// Create a new task manager using the current runtime
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            runtime_handle: tokio::runtime::Handle::current(),
        }
    }

    /// Create a task manager with a specific runtime handle
    pub fn with_runtime(handle: tokio::runtime::Handle) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            runtime_handle: handle,
        }
    }

    /// Spawn a simple task without context
    pub fn spawn<F, T>(&self, name: impl Into<String>, task: F) -> TaskHandle<T>
    where
        F: Future<Output = TaskResult<T>> + Send + 'static,
        T: Send + 'static,
    {
        self.spawn_with_priority(name, TaskPriority::Normal, task)
    }

    /// Spawn a task with specific priority
    pub fn spawn_with_priority<F, T>(
        &self,
        name: impl Into<String>,
        priority: TaskPriority,
        task: F,
    ) -> TaskHandle<T>
    where
        F: Future<Output = TaskResult<T>> + Send + 'static,
        T: Send + 'static,
    {
        let id = self.allocate_id();
        let mut metadata = TaskMetadata::new(id, name).with_priority(priority);
        metadata.started_at = Some(std::time::Instant::now());
        let cancel_token = CancellationToken::new();
        let status = Arc::new(RwLock::new(TaskStatus::Pending));
        let progress = Arc::new(RwLock::new(TaskProgress::default()));
        let (_progress_tx, progress_rx) = mpsc::channel(100);

        // Store task info synchronously so cancel/list cannot miss the task.
        let task_info = TaskInfo {
            metadata: metadata.clone(),
            status: status.clone(),
            cancel_token: cancel_token.clone(),
        };
        self.tasks
            .lock()
            .expect("task registry")
            .insert(id, task_info);

        // Update status to running and spawn task
        let status_clone = status.clone();
        let join_handle = self.runtime_handle.spawn(async move {
            {
                let mut s = status_clone.write().await;
                *s = TaskStatus::Running;
            }

            let result = task.await;
            {
                let mut s = status_clone.write().await;
                *s = match &result {
                    Ok(_) => TaskStatus::Completed,
                    Err(err) if err.is_cancelled() => TaskStatus::Cancelled,
                    Err(_) => TaskStatus::Failed,
                };
            }
            result
        });

        TaskHandle::new(id, status, progress, cancel_token, join_handle, progress_rx)
    }

    /// Spawn a task with context (for progress reporting and cancellation)
    pub fn spawn_with_context<F, Fut, T>(&self, name: impl Into<String>, task: F) -> TaskHandle<T>
    where
        F: FnOnce(TaskContext) -> Fut + Send + 'static,
        Fut: Future<Output = TaskResult<T>> + Send + 'static,
        T: Send + 'static,
    {
        self.spawn_with_context_and_priority(name, TaskPriority::Normal, task)
    }

    /// Spawn a task with context and priority
    pub fn spawn_with_context_and_priority<F, Fut, T>(
        &self,
        name: impl Into<String>,
        priority: TaskPriority,
        task: F,
    ) -> TaskHandle<T>
    where
        F: FnOnce(TaskContext) -> Fut + Send + 'static,
        Fut: Future<Output = TaskResult<T>> + Send + 'static,
        T: Send + 'static,
    {
        let id = self.allocate_id();
        let mut metadata = TaskMetadata::new(id, name).with_priority(priority);
        metadata.started_at = Some(std::time::Instant::now());
        let cancel_token = CancellationToken::new();
        let status = Arc::new(RwLock::new(TaskStatus::Pending));
        let progress = Arc::new(RwLock::new(TaskProgress::default()));
        let (progress_tx, progress_rx) = mpsc::channel(100);

        // Store task info synchronously so cancel/list cannot miss the task.
        let task_info = TaskInfo {
            metadata: metadata.clone(),
            status: status.clone(),
            cancel_token: cancel_token.clone(),
        };
        self.tasks
            .lock()
            .expect("task registry")
            .insert(id, task_info);

        // Create context
        let ctx = TaskContext::new(id, cancel_token.clone(), progress_tx);

        // Update status to running and spawn task
        let status_clone = status.clone();
        let join_handle = self.runtime_handle.spawn(async move {
            {
                let mut s = status_clone.write().await;
                *s = TaskStatus::Running;
            }

            let result = task(ctx).await;
            {
                let mut s = status_clone.write().await;
                *s = match &result {
                    Ok(_) => TaskStatus::Completed,
                    Err(err) if err.is_cancelled() => TaskStatus::Cancelled,
                    Err(_) => TaskStatus::Failed,
                };
            }
            result
        });

        TaskHandle::new(id, status, progress, cancel_token, join_handle, progress_rx)
    }

    /// List all tasks
    pub async fn list_tasks(&self) -> Vec<TaskMetadata> {
        self.tasks
            .lock()
            .expect("task registry")
            .values()
            .map(|info| info.metadata.clone())
            .collect()
    }

    /// Get information about a specific task
    pub async fn get_task(&self, id: TaskId) -> Option<TaskMetadata> {
        self.tasks
            .lock()
            .expect("task registry")
            .get(&id)
            .map(|info| info.metadata.clone())
    }

    /// Get task status
    pub async fn get_task_status(&self, id: TaskId) -> Option<TaskStatus> {
        let status = self
            .tasks
            .lock()
            .expect("task registry")
            .get(&id)
            .map(|info| info.status.clone());
        match status {
            Some(status) => Some(*status.read().await),
            None => None,
        }
    }

    /// Cancel a specific task
    pub async fn cancel_task(&self, id: TaskId) -> bool {
        let found = {
            let tasks = self.tasks.lock().expect("task registry");
            tasks.get(&id).map(|info| {
                info.cancel_token.cancel();
                info.status.clone()
            })
        };
        if let Some(status) = found {
            *status.write().await = TaskStatus::Cancelled;
            true
        } else {
            false
        }
    }

    /// Cancel all tasks
    pub async fn cancel_all(&self) {
        let statuses: Vec<_> = {
            let tasks = self.tasks.lock().expect("task registry");
            tasks
                .values()
                .map(|info| {
                    info.cancel_token.cancel();
                    info.status.clone()
                })
                .collect()
        };
        for status in statuses {
            *status.write().await = TaskStatus::Cancelled;
        }
    }

    /// Remove completed tasks from tracking
    pub async fn cleanup_completed(&self) {
        let mut tasks = self.tasks.lock().expect("task registry");
        tasks.retain(|_, info| {
            // Keep only non-terminal tasks
            let status = info.status.try_read();
            match status {
                Ok(s) => !s.is_terminal(),
                Err(_) => true, // Keep if we can't read (locked)
            }
        });
    }

    /// Get count of active tasks
    pub async fn active_count(&self) -> usize {
        let tasks = self.tasks.lock().expect("task registry");
        let mut count = 0;
        for info in tasks.values() {
            if let Ok(status) = info.status.try_read() {
                if status.is_active() {
                    count += 1;
                }
            }
        }
        count
    }

    /// Get total task count
    pub async fn total_count(&self) -> usize {
        self.tasks.lock().expect("task registry").len()
    }

    /// Allocate a new task ID
    fn allocate_id(&self) -> TaskId {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        TaskId::new(id)
    }
}

impl Default for AsyncTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_spawn_simple_task() {
        let manager = AsyncTaskManager::new();

        let handle = manager.spawn("test-task", async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, crate::async_task::TaskError>(42)
        });

        let result = handle.wait().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_spawn_with_context() {
        let manager = AsyncTaskManager::new();

        let handle = manager.spawn_with_context("context-task", |ctx| async move {
            ctx.report_progress(1, 3, Some("step 1".to_string()))
                .await?;
            tokio::time::sleep(Duration::from_millis(5)).await;

            ctx.report_progress(2, 3, Some("step 2".to_string()))
                .await?;
            tokio::time::sleep(Duration::from_millis(5)).await;

            ctx.report_progress(3, 3, Some("step 3".to_string()))
                .await?;
            Ok::<_, crate::async_task::TaskError>("done")
        });

        let result = handle.wait().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "done");
    }

    #[tokio::test]
    async fn test_cancel_task() {
        let manager = AsyncTaskManager::new();

        let handle = manager.spawn_with_context("cancellable-task", |ctx| async move {
            for i in 0..100 {
                ctx.check_cancellation().await?;
                tokio::time::sleep(Duration::from_millis(10)).await;
                ctx.report_progress(i, 100, None).await?;
            }
            Ok::<_, crate::async_task::TaskError>(())
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        handle.cancel().await;

        let result = handle.wait().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn test_list_tasks() {
        let manager = AsyncTaskManager::new();

        let _handle1 = manager.spawn("task-1", async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok::<_, crate::async_task::TaskError>(())
        });

        let _handle2 = manager.spawn("task-2", async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok::<_, crate::async_task::TaskError>(())
        });

        tokio::time::sleep(Duration::from_millis(10)).await;

        let tasks = manager.list_tasks().await;
        assert_eq!(tasks.len(), 2);

        let names: Vec<_> = tasks.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"task-1"));
        assert!(names.contains(&"task-2"));
    }

    #[tokio::test]
    async fn test_task_priority() {
        let manager = AsyncTaskManager::new();

        let handle = manager.spawn_with_priority("high-priority", TaskPriority::High, async {
            Ok::<_, crate::async_task::TaskError>(())
        });

        tokio::time::sleep(Duration::from_millis(10)).await;

        let tasks = manager.list_tasks().await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].priority, TaskPriority::High);

        handle.wait().await.unwrap();
    }

    #[tokio::test]
    async fn test_cleanup_completed() {
        let manager = AsyncTaskManager::new();

        let handle = manager.spawn("quick-task", async {
            Ok::<_, crate::async_task::TaskError>(())
        });

        handle.wait().await.unwrap();

        assert_eq!(manager.total_count().await, 1);

        manager.cleanup_completed().await;

        assert_eq!(manager.total_count().await, 0);
    }
}
