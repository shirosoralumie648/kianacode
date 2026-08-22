//! Cancellation support for async operations.
//!
//! This module provides tools for gracefully cancelling long-running operations:
//!
//! - [`CancellationToken`]: A token that can be used to signal cancellation
//! - [`CancellationGuard`]: RAII guard for cleanup on cancellation
//! - [`TimeoutToken`]: Automatic cancellation after a timeout
//! - [`TimeoutManager`]: Manages multiple timeout timers
//!
//! # Example
//!
//! ```rust,no_run
//! use kiana_tui::cancellation::{CancellationToken, with_cancellation};
//! use tokio::time::{sleep, Duration};
//!
//! #[tokio::main]
//! async fn main() {
//!     let token = CancellationToken::new();
//!     let token_clone = token.clone();
//!
//!     // Spawn a task that can be cancelled
//!     let handle = tokio::spawn(async move {
//!         match with_cancellation(token_clone, async {
//!             sleep(Duration::from_secs(10)).await;
//!             "completed"
//!         }).await {
//!             Ok(result) => println!("Task completed: {}", result),
//!             Err(e) => println!("Task cancelled: {}", e),
//!         }
//!     });
//!
//!     // Cancel after 1 second
//!     sleep(Duration::from_secs(1)).await;
//!     token.cancel();
//!
//!     handle.await.unwrap();
//! }
//! ```

mod guard;
mod timeout;
mod token;

pub use guard::CancellationGuard;
pub use timeout::{TimeoutManager, TimeoutToken};
pub use token::{CancelledError, CancellationToken};

use std::future::Future;

/// Trait for operations that can be cancelled.
#[async_trait::async_trait]
pub trait Cancellable {
    type Output;
    type Error: From<CancelledError>;

    /// Runs the operation with cancellation support.
    async fn run(&mut self, token: CancellationToken) -> Result<Self::Output, Self::Error>;
}

/// Runs a future with cancellation support.
///
/// Returns `Ok(T)` if the future completes, or `Err(CancelledError)` if cancelled.
///
/// # Example
///
/// ```rust,no_run
/// use kiana_tui::cancellation::{CancellationToken, with_cancellation};
/// use tokio::time::{sleep, Duration};
///
/// #[tokio::main]
/// async fn main() {
///     let token = CancellationToken::new();
///
///     let result = with_cancellation(token, async {
///         sleep(Duration::from_millis(100)).await;
///         42
///     }).await;
///
///     assert_eq!(result.unwrap(), 42);
/// }
/// ```
pub async fn with_cancellation<F, T>(
    token: CancellationToken,
    future: F,
) -> Result<T, CancelledError>
where
    F: Future<Output = T>,
{
    tokio::select! {
        result = future => Ok(result),
        _ = token.cancelled() => Err(CancelledError {
            reason: token.reason(),
        }),
    }
}

/// Runs a future with a timeout.
///
/// Returns `Ok(T)` if the future completes within the timeout,
/// or `Err(CancelledError)` if it times out.
///
/// # Example
///
/// ```rust,no_run
/// use kiana_tui::cancellation::with_timeout;
/// use tokio::time::{sleep, Duration};
///
/// #[tokio::main]
/// async fn main() {
///     let result = with_timeout(Duration::from_millis(50), async {
///         sleep(Duration::from_millis(100)).await;
///         42
///     }).await;
///
///     assert!(result.is_err()); // timed out
/// }
/// ```
pub async fn with_timeout<F, T>(timeout: tokio::time::Duration, future: F) -> Result<T, CancelledError>
where
    F: Future<Output = T>,
{
    let timeout_token = TimeoutToken::new(timeout);
    with_cancellation(timeout_token.token().clone(), future).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_with_cancellation_completes() {
        let token = CancellationToken::new();
        let result = with_cancellation(token, async { 42 }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_with_cancellation_cancelled() {
        let token = CancellationToken::new();
        let token_clone = token.clone();

        let handle = tokio::spawn(async move {
            with_cancellation(token_clone, async {
                sleep(Duration::from_secs(10)).await;
                42
            })
            .await
        });

        sleep(Duration::from_millis(10)).await;
        token.cancel();

        let result = handle.await.unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_with_timeout_completes() {
        let result = with_timeout(Duration::from_millis(100), async {
            sleep(Duration::from_millis(10)).await;
            42
        })
        .await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_with_timeout_times_out() {
        let result = with_timeout(Duration::from_millis(10), async {
            sleep(Duration::from_secs(10)).await;
            42
        })
        .await;
        assert!(result.is_err());
    }
}
