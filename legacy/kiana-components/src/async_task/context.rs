//! Task execution context

use crate::async_task::{
    CancellationToken, ProgressReporter, TaskError, TaskId, TaskProgress, TaskResult,
};
use tokio::sync::mpsc;

/// Context provided to task functions for progress reporting and cancellation
pub struct TaskContext {
    /// Unique task identifier
    pub id: TaskId,
    /// Cancellation token
    pub cancel_token: CancellationToken,
    /// Progress reporter
    reporter: ProgressReporter,
}

impl TaskContext {
    /// Create a new task context
    pub(crate) fn new(
        id: TaskId,
        cancel_token: CancellationToken,
        progress_tx: mpsc::Sender<TaskProgress>,
    ) -> Self {
        Self {
            id,
            cancel_token,
            reporter: ProgressReporter::new(progress_tx),
        }
    }

    /// Report progress (determinate)
    pub async fn report_progress(
        &self,
        current: u64,
        total: u64,
        message: Option<String>,
    ) -> TaskResult<()> {
        self.reporter.report(current, total, message).await
    }

    /// Report indeterminate progress
    pub async fn report_indeterminate(&self, message: Option<String>) -> TaskResult<()> {
        self.reporter.report_indeterminate(message).await
    }

    /// Report completion
    pub async fn report_complete(&self, message: Option<String>) -> TaskResult<()> {
        self.reporter.complete(message).await
    }

    /// Report raw progress
    pub async fn report_raw(&self, progress: TaskProgress) -> TaskResult<()> {
        self.reporter.report_raw(progress).await
    }

    /// Check if cancellation was requested
    pub fn should_cancel(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    /// Check for cancellation and return error if cancelled
    pub async fn check_cancellation(&self) -> TaskResult<()> {
        if self.should_cancel() {
            Err(TaskError::cancelled())
        } else {
            Ok(())
        }
    }

    /// Wait for cancellation signal
    pub async fn cancelled(&self) {
        self.cancel_token.cancelled().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_context_progress_reporting() {
        let (tx, mut rx) = mpsc::channel(10);
        let token = CancellationToken::new();
        let ctx = TaskContext::new(TaskId::new(1), token, tx);

        ctx.report_progress(25, 100, Some("quarter done".to_string()))
            .await
            .unwrap();

        let progress = rx.recv().await.unwrap();
        assert_eq!(progress.current, 25);
        assert_eq!(progress.total, Some(100));
    }

    #[tokio::test]
    async fn test_context_cancellation_check() {
        let (tx, _rx) = mpsc::channel(10);
        let token = CancellationToken::new();
        let ctx = TaskContext::new(TaskId::new(1), token.clone(), tx);

        assert!(!ctx.should_cancel());
        assert!(ctx.check_cancellation().await.is_ok());

        token.cancel();

        assert!(ctx.should_cancel());
        let result = ctx.check_cancellation().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn test_context_wait_cancellation() {
        let (tx, _rx) = mpsc::channel(10);
        let token = CancellationToken::new();
        let ctx = TaskContext::new(TaskId::new(1), token.clone(), tx);

        let handle = tokio::spawn(async move {
            ctx.cancelled().await;
            true
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        token.cancel();

        let result = tokio::time::timeout(tokio::time::Duration::from_millis(100), handle)
            .await
            .unwrap()
            .unwrap();

        assert!(result);
    }
}
