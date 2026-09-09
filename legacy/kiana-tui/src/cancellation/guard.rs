//! RAII guard for cleanup on cancellation.

use super::token::CancellationToken;

/// A guard that performs cleanup when dropped, unless explicitly disarmed.
///
/// This is useful for ensuring resources are cleaned up when an operation
/// is cancelled, while allowing the cleanup to be skipped on success.
pub struct CancellationGuard<F>
where
    F: FnOnce(),
{
    cleanup: Option<F>,
    _token: CancellationToken,
}

impl<F> CancellationGuard<F>
where
    F: FnOnce(),
{
    /// Creates a new cancellation guard.
    ///
    /// The cleanup function will be called when the guard is dropped,
    /// unless `success()` is called first.
    pub fn new(token: CancellationToken, cleanup: F) -> Self {
        Self {
            cleanup: Some(cleanup),
            _token: token,
        }
    }

    /// Disarms the guard, preventing cleanup from running.
    ///
    /// Call this when the operation completes successfully.
    pub fn success(mut self) {
        self.cleanup = None;
    }
}

impl<F> Drop for CancellationGuard<F>
where
    F: FnOnce(),
{
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_cleanup_on_drop() {
        let token = CancellationToken::new();
        let cleaned = Arc::new(AtomicBool::new(false));
        let cleaned_clone = cleaned.clone();

        {
            let _guard = CancellationGuard::new(token, move || {
                cleaned_clone.store(true, Ordering::SeqCst);
            });
        } // guard dropped here

        assert!(cleaned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_success_skips_cleanup() {
        let token = CancellationToken::new();
        let cleaned = Arc::new(AtomicBool::new(false));
        let cleaned_clone = cleaned.clone();

        {
            let guard = CancellationGuard::new(token, move || {
                cleaned_clone.store(true, Ordering::SeqCst);
            });
            guard.success(); // disarm the guard
        }

        assert!(!cleaned.load(Ordering::SeqCst));
    }

    #[test]
    fn test_cleanup_on_panic() {
        let token = CancellationToken::new();
        let cleaned = Arc::new(AtomicBool::new(false));
        let cleaned_clone = cleaned.clone();

        let result = std::panic::catch_unwind(|| {
            let _guard = CancellationGuard::new(token, move || {
                cleaned_clone.store(true, Ordering::SeqCst);
            });
            panic!("test panic");
        });

        assert!(result.is_err());
        assert!(cleaned.load(Ordering::SeqCst));
    }
}
