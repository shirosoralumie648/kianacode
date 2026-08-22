//! Progress reporting for async tasks

use crate::async_task::{TaskProgress, TaskResult};
use tokio::sync::mpsc;

/// Progress reporter for sending progress updates
#[derive(Clone)]
pub struct ProgressReporter {
    tx: mpsc::Sender<TaskProgress>,
}

impl ProgressReporter {
    /// Create a new progress reporter
    pub(crate) fn new(tx: mpsc::Sender<TaskProgress>) -> Self {
        Self { tx }
    }

    /// Report determinate progress (known total)
    pub async fn report(
        &self,
        current: u64,
        total: u64,
        message: Option<String>,
    ) -> TaskResult<()> {
        let mut progress = TaskProgress::new(current, total);
        if let Some(msg) = message {
            progress = progress.with_message(msg);
        }

        self.tx
            .send(progress)
            .await
            .map_err(|_| "progress channel closed".into())
    }

    /// Report indeterminate progress (unknown total)
    pub async fn report_indeterminate(&self, message: Option<String>) -> TaskResult<()> {
        let mut progress = TaskProgress::indeterminate();
        if let Some(msg) = message {
            progress = progress.with_message(msg);
        }

        self.tx
            .send(progress)
            .await
            .map_err(|_| "progress channel closed".into())
    }

    /// Report completion
    pub async fn complete(&self, message: Option<String>) -> TaskResult<()> {
        let mut progress = TaskProgress::completed();
        if let Some(msg) = message {
            progress = progress.with_message(msg);
        }

        self.tx
            .send(progress)
            .await
            .map_err(|_| "progress channel closed".into())
    }

    /// Report progress with custom TaskProgress
    pub async fn report_raw(&self, progress: TaskProgress) -> TaskResult<()> {
        self.tx
            .send(progress)
            .await
            .map_err(|_| "progress channel closed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_report_determinate() {
        let (tx, mut rx) = mpsc::channel(10);
        let reporter = ProgressReporter::new(tx);

        reporter
            .report(50, 100, Some("halfway".to_string()))
            .await
            .unwrap();

        let progress = rx.recv().await.unwrap();
        assert_eq!(progress.current, 50);
        assert_eq!(progress.total, Some(100));
        assert_eq!(progress.percentage, Some(50.0));
        assert_eq!(progress.message, Some("halfway".to_string()));
    }

    #[tokio::test]
    async fn test_report_indeterminate() {
        let (tx, mut rx) = mpsc::channel(10);
        let reporter = ProgressReporter::new(tx);

        reporter
            .report_indeterminate(Some("processing...".to_string()))
            .await
            .unwrap();

        let progress = rx.recv().await.unwrap();
        assert_eq!(progress.total, None);
        assert_eq!(progress.percentage, None);
        assert_eq!(progress.message, Some("processing...".to_string()));
    }

    #[tokio::test]
    async fn test_report_complete() {
        let (tx, mut rx) = mpsc::channel(10);
        let reporter = ProgressReporter::new(tx);

        reporter.complete(Some("done!".to_string())).await.unwrap();

        let progress = rx.recv().await.unwrap();
        assert!(progress.is_complete());
        assert_eq!(progress.message, Some("done!".to_string()));
    }

    #[tokio::test]
    async fn test_channel_closed() {
        let (tx, rx) = mpsc::channel(10);
        let reporter = ProgressReporter::new(tx);

        // Drop receiver to close channel
        drop(rx);

        let result = reporter.report(1, 10, None).await;
        assert!(result.is_err());
    }
}
