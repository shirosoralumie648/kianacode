//! Background task types and metadata.

use crate::cancellation::CancellationToken;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Unique identifier for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(Uuid);

impl TaskId {
    /// Creates a new random task ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a background task.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    /// Task is waiting to be executed.
    Pending,
    /// Task is currently running.
    Running {
        /// Progress from 0.0 to 1.0, if known.
        progress: Option<f32>,
        /// Current status message.
        message: Option<String>,
    },
    /// Task completed successfully.
    Completed { duration: Duration },
    /// Task failed with an error.
    Failed {
        error: String,
        duration: Duration,
    },
    /// Task was cancelled.
    Cancelled { reason: Option<String> },
}

impl TaskStatus {
    /// Returns true if the task is done (completed, failed, or cancelled).
    pub fn is_done(&self) -> bool {
        matches!(
            self,
            TaskStatus::Completed { .. } | TaskStatus::Failed { .. } | TaskStatus::Cancelled { .. }
        )
    }

    /// Returns true if the task is running.
    pub fn is_running(&self) -> bool {
        matches!(self, TaskStatus::Running { .. })
    }
}

/// Metadata about a background task.
#[derive(Debug, Clone)]
pub struct TaskMetadata {
    /// Unique task identifier.
    pub id: TaskId,
    /// Task name.
    pub name: String,
    /// When the task was created.
    pub created_at: Instant,
    /// When the task started running.
    pub started_at: Option<Instant>,
    /// When the task completed.
    pub completed_at: Option<Instant>,
}

impl TaskMetadata {
    /// Creates new task metadata.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: TaskId::new(),
            name: name.into(),
            created_at: Instant::now(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Gets the duration the task has been running, if started.
    pub fn running_duration(&self) -> Option<Duration> {
        self.started_at.map(|start| start.elapsed())
    }

    /// Gets the total duration from creation to completion.
    pub fn total_duration(&self) -> Option<Duration> {
        self.completed_at
            .map(|end| end.duration_since(self.created_at))
    }
}

/// Progress update for a task.
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    /// Progress from 0.0 to 1.0, if known.
    pub progress: Option<f32>,
    /// Status message.
    pub message: Option<String>,
}

/// Sender for task progress updates.
#[derive(Clone)]
pub struct ProgressSender {
    tx: tokio::sync::mpsc::UnboundedSender<ProgressUpdate>,
}

impl ProgressSender {
    /// Creates a new progress sender.
    pub(crate) fn new(tx: tokio::sync::mpsc::UnboundedSender<ProgressUpdate>) -> Self {
        Self { tx }
    }

    /// Updates the task progress.
    ///
    /// Progress should be between 0.0 and 1.0.
    pub fn update(&self, progress: f32, message: Option<String>) {
        let _ = self.tx.send(ProgressUpdate {
            progress: Some(progress.clamp(0.0, 1.0)),
            message,
        });
    }

    /// Updates the task progress with a message.
    pub fn message(&self, message: impl Into<String>) {
        let _ = self.tx.send(ProgressUpdate {
            progress: None,
            message: Some(message.into()),
        });
    }

    /// Sets the progress to a specific value.
    pub fn set_progress(&self, progress: f32) {
        let _ = self.tx.send(ProgressUpdate {
            progress: Some(progress.clamp(0.0, 1.0)),
            message: None,
        });
    }
}

/// Result type for background tasks.
pub type TaskResult<T> = Result<T, anyhow::Error>;

/// Trait for background tasks.
#[async_trait::async_trait]
pub trait BackgroundTask: Send {
    type Output: Send;

    /// Returns the name of this task.
    fn name(&self) -> &str;

    /// Executes the task.
    ///
    /// The task should periodically check the cancellation token and
    /// report progress using the progress sender.
    async fn execute(
        &mut self,
        token: CancellationToken,
        progress: ProgressSender,
    ) -> TaskResult<Self::Output>;

    /// Called when the task is cancelled.
    ///
    /// This is optional and can be used for cleanup.
    async fn on_cancel(&mut self) {}
}

/// A background task created from an async function or closure.
pub struct FunctionTask<F, Fut, T>
where
    F: FnOnce(CancellationToken, ProgressSender) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = TaskResult<T>> + Send + 'static,
    T: Send + 'static,
{
    name: String,
    func: Option<F>,
    _phantom: std::marker::PhantomData<fn() -> (Fut, T)>,
}

impl<F, Fut, T> FunctionTask<F, Fut, T>
where
    F: FnOnce(CancellationToken, ProgressSender) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = TaskResult<T>> + Send + 'static,
    T: Send + 'static,
{
    /// Creates a new function task.
    pub fn new(name: impl Into<String>, func: F) -> Self {
        Self {
            name: name.into(),
            func: Some(func),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<F, Fut, T> BackgroundTask for FunctionTask<F, Fut, T>
where
    F: FnOnce(CancellationToken, ProgressSender) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = TaskResult<T>> + Send + 'static,
    T: Send + 'static,
{
    type Output = T;

    fn name(&self) -> &str {
        &self.name
    }

    async fn execute(
        &mut self,
        token: CancellationToken,
        progress: ProgressSender,
    ) -> TaskResult<Self::Output> {
        let func = self.func.take().expect("Task already executed");
        func(token, progress).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_id() {
        let id1 = TaskId::new();
        let id2 = TaskId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_task_status_is_done() {
        assert!(!TaskStatus::Pending.is_done());
        assert!(!TaskStatus::Running {
            progress: None,
            message: None
        }
        .is_done());
        assert!(TaskStatus::Completed {
            duration: Duration::from_secs(1)
        }
        .is_done());
        assert!(TaskStatus::Failed {
            error: "test".to_string(),
            duration: Duration::from_secs(1)
        }
        .is_done());
        assert!(TaskStatus::Cancelled { reason: None }.is_done());
    }

    #[test]
    fn test_task_metadata() {
        let metadata = TaskMetadata::new("test task");
        assert_eq!(metadata.name, "test task");
        assert!(metadata.started_at.is_none());
        assert!(metadata.completed_at.is_none());
    }

    #[test]
    fn test_progress_sender() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = ProgressSender::new(tx);

        sender.update(0.5, Some("halfway".to_string()));
        let update = rx.try_recv().unwrap();
        assert_eq!(update.progress, Some(0.5));
        assert_eq!(update.message, Some("halfway".to_string()));

        sender.message("test message");
        let update = rx.try_recv().unwrap();
        assert_eq!(update.progress, None);
        assert_eq!(update.message, Some("test message".to_string()));

        sender.set_progress(0.75);
        let update = rx.try_recv().unwrap();
        assert_eq!(update.progress, Some(0.75));
        assert_eq!(update.message, None);
    }

    #[test]
    fn test_progress_clamping() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = ProgressSender::new(tx);

        sender.set_progress(-0.5);
        assert_eq!(rx.try_recv().unwrap().progress, Some(0.0));

        sender.set_progress(1.5);
        assert_eq!(rx.try_recv().unwrap().progress, Some(1.0));
    }

    #[tokio::test]
    async fn test_function_task() {
        let task = FunctionTask::new("test", |_token, progress| async move {
            progress.message("starting");
            progress.set_progress(0.5);
            progress.message("done");
            Ok(42)
        });

        assert_eq!(task.name(), "test");
    }
}
