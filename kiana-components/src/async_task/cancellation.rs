//! Cancellation token for async tasks

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;

/// Token for cancelling async operations
#[derive(Clone)]
pub struct CancellationToken {
    inner: Arc<CancellationTokenInner>,
}

struct CancellationTokenInner {
    cancelled: AtomicBool,
    tx: watch::Sender<bool>,
    rx: watch::Receiver<bool>,
}

impl CancellationToken {
    /// Create a new cancellation token
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self {
            inner: Arc::new(CancellationTokenInner {
                cancelled: AtomicBool::new(false),
                tx,
                rx,
            }),
        }
    }

    /// Cancel the operation
    pub fn cancel(&self) {
        if !self.is_cancelled() {
            self.inner.cancelled.store(true, Ordering::SeqCst);
            let _ = self.inner.tx.send(true);
        }
    }

    /// Check if cancellation was requested
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Wait for cancellation signal
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        let mut rx = self.inner.rx.clone();
        let _ = rx.changed().await;
    }

    /// Create a child token that is cancelled when parent is cancelled
    pub fn child(&self) -> Self {
        let child = Self::new();
        let child_clone = child.clone();
        let parent = self.clone();

        tokio::spawn(async move {
            parent.cancelled().await;
            child_clone.cancel();
        });

        child
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_cancellation_token() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());

        token.cancel();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellation_signal() {
        let token = CancellationToken::new();
        let token_clone = token.clone();

        let handle = tokio::spawn(async move {
            token_clone.cancelled().await;
            true
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        token.cancel();

        let result = tokio::time::timeout(Duration::from_millis(100), handle)
            .await
            .expect("task should complete")
            .expect("task should not panic");

        assert!(result);
    }

    #[tokio::test]
    async fn test_child_token() {
        let parent = CancellationToken::new();
        let child = parent.child();

        assert!(!parent.is_cancelled());
        assert!(!child.is_cancelled());

        parent.cancel();

        // Give child time to react to parent cancellation
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(parent.is_cancelled());
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn test_already_cancelled() {
        let token = CancellationToken::new();
        token.cancel();

        // Should return immediately
        tokio::time::timeout(Duration::from_millis(10), token.cancelled())
            .await
            .expect("should not timeout");
    }
}
