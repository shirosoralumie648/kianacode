//! Task handle for tracking and controlling background tasks.

use super::task::{TaskId, TaskMetadata, TaskResult, TaskStatus};
use crate::cancellation::CancellationToken;
use std::sync::{Arc, RwLock};
use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};

/// A handle to a background task.
///
/// This handle can be used to:
/// - Query the task's status and metadata
/// - Wait for the task to complete
/// - Cancel the task
pub struct TaskHandle<T> {
    id: TaskId,
    metadata: Arc<RwLock<TaskMetadata>>,
    status: Arc<RwLock<TaskStatus>>,
    result_rx: Option<oneshot::Receiver<TaskResult<T>>>,
    cancel_token: CancellationToken,
}

impl<T> TaskHandle<T> {
    /// Creates a new task handle.
    pub(crate) fn new(
        id: TaskId,
        metadata: Arc<RwLock<TaskMetadata>>,
        status: Arc<RwLock<TaskStatus>>,
        result_rx: oneshot::Receiver<TaskResult<T>>,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            id,
            metadata,
            status,
            result_rx: Some(result_rx),
            cancel_token,
        }
    }

    /// Gets the task ID.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Gets the current task status.
    pub fn status(&self) -> TaskStatus {
        self.status.read().unwrap().clone()
    }

    /// Gets the task metadata.
    pub fn metadata(&self) -> TaskMetadata {
        self.metadata.read().unwrap().clone()
    }

    /// Checks if the task is done (completed, failed, or cancelled).
    pub fn is_done(&self) -> bool {
        self.status().is_done()
    }

    /// Cancels the task.
    ///
    /// Returns an error if the task is already done.
    pub fn cancel(&self) -> anyhow::Result<()> {
        if self.is_done() {
            anyhow::bail!("Task is already done");
        }
        self.cancel_token.cancel();
        Ok(())
    }

    /// Waits for the task to complete and returns the result.
    ///
    /// This consumes the handle.
    pub async fn await_result(mut self) -> TaskResult<T> {
        match self.result_rx.take() {
            Some(rx) => rx.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!("Task was dropped before completing"))
            }),
            None => Err(anyhow::anyhow!("Result already consumed")),
        }
    }

    /// Waits for the task to complete with a timeout.
    ///
    /// Returns `Ok(result)` if the task completes within the timeout,
    /// or `Err(TimeoutError)` if it times out.
    pub async fn await_result_timeout(
        mut self,
        duration: Duration,
    ) -> Result<TaskResult<T>, TimeoutError> {
        match self.result_rx.take() {
            Some(rx) => match timeout(duration, rx).await {
                Ok(Ok(result)) => Ok(result),
                Ok(Err(_)) => Ok(Err(anyhow::anyhow!("Task was dropped before completing"))),
                Err(_) => Err(TimeoutError),
            },
            None => Ok(Err(anyhow::anyhow!("Result already consumed"))),
        }
    }
}

/// Error returned when a task times out.
#[derive(Debug, Clone, Copy)]
pub struct TimeoutError;

impl std::fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Task timed out")
    }
}

impl std::error::Error for TimeoutError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_task_handle_basic() {
        let id = TaskId::new();
        let metadata = Arc::new(RwLock::new(TaskMetadata::new("test")));
        let status = Arc::new(RwLock::new(TaskStatus::Pending));
        let (tx, rx) = oneshot::channel();
        let token = CancellationToken::new();

        let handle: TaskHandle<i32> = TaskHandle::new(id, metadata, status, rx, token);

        assert_eq!(handle.id(), id);
        assert_eq!(handle.status(), TaskStatus::Pending);
        assert!(!handle.is_done());

        tx.send(Ok(42)).unwrap();
        let result = handle.await_result().await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_task_handle_cancel() {
        let id = TaskId::new();
        let metadata = Arc::new(RwLock::new(TaskMetadata::new("test")));
        let status = Arc::new(RwLock::new(TaskStatus::Running {
            progress: None,
            message: None,
        }));
        let (_tx, rx) = oneshot::channel::<TaskResult<i32>>();
        let token = CancellationToken::new();

        let handle = TaskHandle::new(id, metadata, status, rx, token.clone());

        assert!(!token.is_cancelled());
        handle.cancel().unwrap();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn test_task_handle_timeout() {
        let id = TaskId::new();
        let metadata = Arc::new(RwLock::new(TaskMetadata::new("test")));
        let status = Arc::new(RwLock::new(TaskStatus::Running {
            progress: None,
            message: None,
        }));
        let (_tx, rx) = oneshot::channel::<TaskResult<i32>>();
        let token = CancellationToken::new();

        let handle = TaskHandle::new(id, metadata, status, rx, token);

        let result = handle
            .await_result_timeout(Duration::from_millis(10))
            .await;
        assert!(result.is_err());
    }
}
