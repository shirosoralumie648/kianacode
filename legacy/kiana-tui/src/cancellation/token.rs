//! Cancellation token implementation.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::Notify;

/// A token that can be used to signal cancellation.
#[derive(Clone)]
pub struct CancellationToken {
    inner: Arc<TokenInner>,
}

struct TokenInner {
    /// Whether this token has been cancelled
    cancelled: AtomicBool,
    /// Optional cancellation reason
    reason: Mutex<Option<String>>,
    /// Callbacks to invoke on cancellation
    callbacks: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    /// Child tokens that will be cancelled when this token is cancelled
    children: Mutex<Vec<CancellationToken>>,
    /// Notification for async waiting
    notify: Notify,
}

impl CancellationToken {
    /// Creates a new cancellation token.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TokenInner {
                cancelled: AtomicBool::new(false),
                reason: Mutex::new(None),
                callbacks: Mutex::new(Vec::new()),
                children: Mutex::new(Vec::new()),
                notify: Notify::new(),
            }),
        }
    }

    /// Creates a child token that will be cancelled when this token is cancelled.
    pub fn child_token(&self) -> Self {
        let child = Self::new();

        // If parent is already cancelled, cancel child immediately
        if self.is_cancelled() {
            child.cancel_with_reason(
                self.reason()
                    .unwrap_or_else(|| "Parent token cancelled".to_string()),
            );
        } else {
            // Register child with parent
            self.inner.children.lock().unwrap().push(child.clone());
        }

        child
    }

    /// Checks if this token has been cancelled.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    /// Cancels this token with no specific reason.
    pub fn cancel(&self) {
        self.cancel_with_reason("Operation cancelled");
    }

    /// Cancels this token with a specific reason.
    pub fn cancel_with_reason(&self, reason: impl Into<String>) {
        // Set cancelled flag
        if self
            .inner
            .cancelled
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Acquire)
            .is_err()
        {
            // Already cancelled
            return;
        }

        // Set reason
        *self.inner.reason.lock().unwrap() = Some(reason.into());

        // Notify waiters
        self.inner.notify.notify_waiters();

        // Cancel all children
        let children = self.inner.children.lock().unwrap().clone();
        for child in children {
            child.cancel_with_reason("Parent token cancelled");
        }

        // Invoke callbacks
        let callbacks = std::mem::take(&mut *self.inner.callbacks.lock().unwrap());
        for callback in callbacks {
            callback();
        }
    }

    /// Gets the cancellation reason if this token has been cancelled.
    pub fn reason(&self) -> Option<String> {
        self.inner.reason.lock().unwrap().clone()
    }

    /// Registers a callback to be invoked when this token is cancelled.
    ///
    /// If the token is already cancelled, the callback is invoked immediately.
    pub fn on_cancel<F>(&self, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        if self.is_cancelled() {
            callback();
        } else {
            self.inner
                .callbacks
                .lock()
                .unwrap()
                .push(Box::new(callback));
        }
    }

    /// Waits asynchronously until this token is cancelled.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        self.inner.notify.notified().await;
    }

    /// Throws a cancellation error if this token has been cancelled.
    pub fn throw_if_cancelled(&self) -> Result<(), CancelledError> {
        if self.is_cancelled() {
            Err(CancelledError {
                reason: self.reason(),
            })
        } else {
            Ok(())
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Error returned when an operation is cancelled.
#[derive(Debug, Clone)]
pub struct CancelledError {
    pub reason: Option<String>,
}

impl std::fmt::Display for CancelledError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.reason {
            Some(reason) => write!(f, "Operation cancelled: {}", reason),
            None => write!(f, "Operation cancelled"),
        }
    }
}

impl std::error::Error for CancelledError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::time::{sleep, Duration};

    #[test]
    fn test_basic_cancellation() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());

        token.cancel();
        assert!(token.is_cancelled());
        assert!(token.reason().is_some());
    }

    #[test]
    fn test_cancel_with_reason() {
        let token = CancellationToken::new();
        token.cancel_with_reason("timeout");

        assert!(token.is_cancelled());
        assert_eq!(token.reason(), Some("timeout".to_string()));
    }

    #[test]
    fn test_child_token() {
        let parent = CancellationToken::new();
        let child = parent.child_token();

        assert!(!parent.is_cancelled());
        assert!(!child.is_cancelled());

        parent.cancel();
        assert!(parent.is_cancelled());
        assert!(child.is_cancelled());
    }

    #[test]
    fn test_child_token_already_cancelled() {
        let parent = CancellationToken::new();
        parent.cancel_with_reason("early cancel");

        let child = parent.child_token();
        assert!(child.is_cancelled());
    }

    #[test]
    fn test_callback() {
        let token = CancellationToken::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        token.on_cancel(move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert!(!called.load(Ordering::SeqCst));
        token.cancel();
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_callback_already_cancelled() {
        let token = CancellationToken::new();
        token.cancel();

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        token.on_cancel(move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_multiple_callbacks() {
        let token = CancellationToken::new();
        let count = Arc::new(AtomicUsize::new(0));

        for _ in 0..5 {
            let count_clone = count.clone();
            token.on_cancel(move || {
                count_clone.fetch_add(1, Ordering::SeqCst);
            });
        }

        token.cancel();
        assert_eq!(count.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_throw_if_cancelled() {
        let token = CancellationToken::new();
        assert!(token.throw_if_cancelled().is_ok());

        token.cancel_with_reason("test");
        let result = token.throw_if_cancelled();
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.reason, Some("test".to_string()));
    }

    #[tokio::test]
    async fn test_async_wait() {
        let token = CancellationToken::new();
        let token_clone = token.clone();

        let handle = tokio::spawn(async move {
            token_clone.cancelled().await;
        });

        sleep(Duration::from_millis(10)).await;
        assert!(!handle.is_finished());

        token.cancel();
        sleep(Duration::from_millis(10)).await;
        assert!(handle.is_finished());
    }

    #[tokio::test]
    async fn test_cancelled_already() {
        let token = CancellationToken::new();
        token.cancel();

        // Should return immediately
        token.cancelled().await;
    }

    #[test]
    fn test_multiple_cancels() {
        let token = CancellationToken::new();
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();

        token.on_cancel(move || {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        token.cancel();
        token.cancel();
        token.cancel();

        // Callback should only be called once
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
