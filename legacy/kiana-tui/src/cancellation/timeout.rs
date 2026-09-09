//! Timeout support for cancellation.

use super::token::CancellationToken;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

/// A cancellation token that will be automatically cancelled after a timeout.
pub struct TimeoutToken {
    token: CancellationToken,
    _timer: JoinHandle<()>,
}

impl TimeoutToken {
    /// Creates a new timeout token that will be cancelled after the specified duration.
    pub fn new(timeout: Duration) -> Self {
        let token = CancellationToken::new();
        let token_clone = token.clone();

        let timer = tokio::spawn(async move {
            sleep(timeout).await;
            token_clone.cancel_with_reason(format!("Timeout after {:?}", timeout));
        });

        Self {
            token,
            _timer: timer,
        }
    }

    /// Gets the underlying cancellation token.
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }

    /// Cancels the token immediately.
    pub fn cancel(&self) {
        self.token.cancel();
    }
}

impl Drop for TimeoutToken {
    fn drop(&mut self) {
        self._timer.abort();
    }
}

/// Manages multiple timeout timers.
pub struct TimeoutManager {
    timers: Arc<Mutex<HashMap<Uuid, JoinHandle<()>>>>,
}

impl TimeoutManager {
    /// Creates a new timeout manager.
    pub fn new() -> Self {
        Self {
            timers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Sets a timeout that will invoke the callback after the specified duration.
    ///
    /// Returns a unique ID that can be used to cancel the timeout.
    pub fn set_timeout<F>(&self, duration: Duration, callback: F) -> Uuid
    where
        F: FnOnce() + Send + 'static,
    {
        let id = Uuid::new_v4();
        let timers = self.timers.clone();
        let timer_id = id;

        let handle = tokio::spawn(async move {
            sleep(duration).await;
            callback();
            timers.lock().unwrap().remove(&timer_id);
        });

        self.timers.lock().unwrap().insert(id, handle);
        id
    }

    /// Cancels a previously set timeout.
    pub fn cancel_timeout(&self, id: Uuid) {
        if let Some(handle) = self.timers.lock().unwrap().remove(&id) {
            handle.abort();
        }
    }

    /// Creates a cancellation token that will be cancelled after the specified duration.
    pub fn create_timeout_token(&self, duration: Duration) -> CancellationToken {
        let token = CancellationToken::new();
        let token_clone = token.clone();

        let id = self.set_timeout(duration, move || {
            token_clone.cancel_with_reason(format!("Timeout after {:?}", duration));
        });

        // Store the timer ID so we can cancel it if the token is dropped early
        let timers = self.timers.clone();
        token.on_cancel(move || {
            timers.lock().unwrap().remove(&id);
        });

        token
    }

    /// Clears all pending timeouts.
    pub fn clear(&self) {
        let mut timers = self.timers.lock().unwrap();
        for (_, handle) in timers.drain() {
            handle.abort();
        }
    }
}

impl Default for TimeoutManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TimeoutManager {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_timeout_token() {
        let timeout = TimeoutToken::new(Duration::from_millis(50));
        assert!(!timeout.token().is_cancelled());

        sleep(Duration::from_millis(100)).await;
        assert!(timeout.token().is_cancelled());
        assert!(timeout.token().reason().unwrap().contains("Timeout"));
    }

    #[tokio::test]
    async fn test_timeout_token_early_cancel() {
        let timeout = TimeoutToken::new(Duration::from_millis(100));
        timeout.cancel();

        assert!(timeout.token().is_cancelled());
    }

    #[tokio::test]
    async fn test_timeout_manager_basic() {
        let manager = TimeoutManager::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        manager.set_timeout(Duration::from_millis(50), move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        sleep(Duration::from_millis(100)).await;
        assert!(called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_timeout_manager_cancel() {
        let manager = TimeoutManager::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let id = manager.set_timeout(Duration::from_millis(50), move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        manager.cancel_timeout(id);
        sleep(Duration::from_millis(100)).await;
        assert!(!called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_timeout_manager_token() {
        let manager = TimeoutManager::new();
        let token = manager.create_timeout_token(Duration::from_millis(50));

        assert!(!token.is_cancelled());
        sleep(Duration::from_millis(100)).await;
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn test_timeout_manager_clear() {
        let manager = TimeoutManager::new();
        let called1 = Arc::new(AtomicBool::new(false));
        let called2 = Arc::new(AtomicBool::new(false));
        let called1_clone = called1.clone();
        let called2_clone = called2.clone();

        manager.set_timeout(Duration::from_millis(50), move || {
            called1_clone.store(true, Ordering::SeqCst);
        });
        manager.set_timeout(Duration::from_millis(50), move || {
            called2_clone.store(true, Ordering::SeqCst);
        });

        manager.clear();
        sleep(Duration::from_millis(100)).await;
        assert!(!called1.load(Ordering::SeqCst));
        assert!(!called2.load(Ordering::SeqCst));
    }
}
