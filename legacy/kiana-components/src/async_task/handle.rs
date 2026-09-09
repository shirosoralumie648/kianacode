//! Task handle for interacting with spawned tasks

use crate::async_task::{
    CancellationToken, TaskError, TaskId, TaskProgress, TaskResult, TaskStatus,
};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

/// Handle for interacting with a spawned task
pub struct TaskHandle<T> {
    id: TaskId,
    status: Arc<RwLock<TaskStatus>>,
    progress: Arc<RwLock<TaskProgress>>,
    cancel_token: CancellationToken,
    join_handle: Option<JoinHandle<TaskResult<T>>>,
}

impl<T> TaskHandle<T> {
    /// Create a new task handle
    pub(crate) fn new(
        id: TaskId,
        status: Arc<RwLock<TaskStatus>>,
        progress: Arc<RwLock<TaskProgress>>,
        cancel_token: CancellationToken,
        join_handle: JoinHandle<TaskResult<T>>,
        progress_rx: mpsc::Receiver<TaskProgress>,
    ) -> Self {
        // Own the receiver in a background task so poll/wait never share it.
        let progress_clone = progress.clone();
        tokio::spawn(async move {
            let mut progress_rx = progress_rx;
            while let Some(new_progress) = progress_rx.recv().await {
                *progress_clone.write().await = new_progress;
            }
        });

        Self {
            id,
            status,
            progress,
            cancel_token,
            join_handle: Some(join_handle),
        }
    }

    /// Get the task ID
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Get the current task status
    pub async fn status(&self) -> TaskStatus {
        *self.status.read().await
    }

    /// Get the current task progress
    pub async fn progress(&self) -> TaskProgress {
        self.progress.read().await.clone()
    }

    /// Check if the task is cancelled
    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Cancel the task
    pub async fn cancel(&self) {
        self.cancel_token.cancel();
        *self.status.write().await = TaskStatus::Cancelled;
    }

    /// Wait for the task to complete and return the result
    pub async fn wait(mut self) -> TaskResult<T> {
        if let Some(handle) = self.join_handle.take() {
            match handle.await {
                Ok(result) => {
                    if result.is_ok() {
                        *self.status.write().await = TaskStatus::Completed;
                    } else {
                        *self.status.write().await = TaskStatus::Failed;
                    }
                    result
                }
                Err(e) => {
                    *self.status.write().await = TaskStatus::Failed;
                    Err(TaskError::runtime(format!("task panicked: {}", e)))
                }
            }
        } else {
            Err(TaskError::runtime("task already consumed"))
        }
    }

    /// Poll the latest progress snapshot (non-blocking)
    pub async fn poll_progress(&self) -> Option<TaskProgress> {
        Some(self.progress.read().await.clone())
    }

    /// Wait for the next progress snapshot change
    pub async fn next_progress(&self) -> Option<TaskProgress> {
        let current = self.progress.read().await.clone();
        loop {
            if self.status().await.is_terminal() {
                let latest = self.progress.read().await.clone();
                return if latest.current != current.current
                    || latest.total != current.total
                    || latest.message != current.message
                {
                    Some(latest)
                } else {
                    None
                };
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let latest = self.progress.read().await.clone();
            if latest.current != current.current
                || latest.total != current.total
                || latest.message != current.message
            {
                return Some(latest);
            }
        }
    }

    /// Check if task is complete (terminal state)
    pub async fn is_complete(&self) -> bool {
        self.status().await.is_terminal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_task_handle_wait() {
        let id = TaskId::new(1);
        let status = Arc::new(RwLock::new(TaskStatus::Running));
        let progress = Arc::new(RwLock::new(TaskProgress::default()));
        let cancel_token = CancellationToken::new();
        let (progress_tx, progress_rx) = mpsc::channel(10);

        let join_handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<_, TaskError>(42)
        });

        drop(progress_tx); // Close progress channel

        let handle = TaskHandle::new(
            id,
            status.clone(),
            progress,
            cancel_token,
            join_handle,
            progress_rx,
        );

        let result = handle.wait().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(*status.read().await, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn test_task_handle_cancel() {
        let id = TaskId::new(1);
        let status = Arc::new(RwLock::new(TaskStatus::Running));
        let progress = Arc::new(RwLock::new(TaskProgress::default()));
        let cancel_token = CancellationToken::new();
        let (_progress_tx, progress_rx) = mpsc::channel(10);

        let token_clone = cancel_token.clone();
        let join_handle = tokio::spawn(async move {
            token_clone.cancelled().await;
            Err::<(), _>(TaskError::cancelled())
        });

        let handle = TaskHandle::new(
            id,
            status.clone(),
            progress,
            cancel_token,
            join_handle,
            progress_rx,
        );

        assert!(!handle.is_cancelled());
        handle.cancel().await;
        assert!(handle.is_cancelled());
        assert_eq!(*status.read().await, TaskStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_task_handle_progress() {
        let id = TaskId::new(1);
        let status = Arc::new(RwLock::new(TaskStatus::Running));
        let progress = Arc::new(RwLock::new(TaskProgress::default()));
        let cancel_token = CancellationToken::new();
        let (progress_tx, progress_rx) = mpsc::channel(10);

        let join_handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok::<_, TaskError>(())
        });

        let handle = TaskHandle::new(
            id,
            status,
            progress.clone(),
            cancel_token,
            join_handle,
            progress_rx,
        );

        // Send progress update
        let new_progress = TaskProgress::new(50, 100);
        progress_tx.send(new_progress).await.unwrap();

        // Poll for progress - give background task time to update
        tokio::time::sleep(Duration::from_millis(10)).await;
        let current = handle.progress().await;
        assert_eq!(current.current, 50);

        handle.cancel().await;
    }
}
