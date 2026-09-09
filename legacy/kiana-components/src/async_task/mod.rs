//! Async task management system
//!
//! This module provides a Tokio-based async task manager for spawning, tracking,
//! and managing background tasks with progress reporting and cancellation support.
//!
//! # Features
//!
//! - **Task Spawning**: Create async tasks with or without context
//! - **Progress Reporting**: Report determinate and indeterminate progress
//! - **Cancellation**: Gracefully cancel running tasks
//! - **Error Handling**: Comprehensive error types and propagation
//! - **Task Tracking**: List and query task status and metadata
//!
//! # Example
//!
//! ```rust,no_run
//! use kiana_components::async_task::{AsyncTaskManager, TaskError};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), TaskError> {
//!     let manager = AsyncTaskManager::new();
//!
//!     // Spawn a simple task
//!     let handle = manager.spawn("fetch-data", async {
//!         // Simulate work
//!         tokio::time::sleep(std::time::Duration::from_millis(100)).await;
//!         Ok::<_, TaskError>("data".to_string())
//!     });
//!
//!     // Wait for completion
//!     let result = handle.wait().await?;
//!     println!("Got: {}", result);
//!
//!     Ok(())
//! }
//! ```
//!
//! # Example with Context
//!
//! ```rust
//! use kiana_components::async_task::{AsyncTaskManager, TaskError};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), TaskError> {
//!     let manager = AsyncTaskManager::new();
//!
//!     let handle = manager.spawn_with_context("process-files", |ctx| async move {
//!         for i in 0..10 {
//!             // Check for cancellation
//!             ctx.check_cancellation().await?;
//!
//!             // Do work
//!             tokio::time::sleep(std::time::Duration::from_millis(10)).await;
//!
//!             // Report progress
//!             ctx.report_progress(i + 1, 10, Some(format!("Processing item {}", i))).await?;
//!         }
//!
//!         Ok::<_, TaskError>(10)
//!     });
//!
//!     let result = handle.wait().await?;
//!     println!("Processed {} items", result);
//!
//!     Ok(())
//! }
//! ```

mod cancellation;
mod context;
mod error;
mod handle;
mod manager;
mod progress;
mod types;

pub use cancellation::CancellationToken;
pub use context::TaskContext;
pub use error::{ErrorKind, TaskError, TaskResult};
pub use handle::TaskHandle;
pub use manager::AsyncTaskManager;
pub use progress::ProgressReporter;
pub use types::{TaskId, TaskMetadata, TaskPriority, TaskProgress, TaskStatus};
