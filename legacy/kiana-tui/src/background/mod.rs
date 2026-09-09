//! Background task execution system.
//!
//! This module provides infrastructure for running long-running operations
//! in the background without blocking the UI.
//!
//! # Example
//!
//! ```rust,no_run
//! use kiana_tui::background::{TaskExecutor, FunctionTask};
//! use tokio::time::{sleep, Duration};
//!
//! #[tokio::main]
//! async fn main() {
//!     let executor = TaskExecutor::new();
//!
//!     let task = FunctionTask::new("my-task", |token, progress| async move {
//!         progress.message("Starting...");
//!         progress.set_progress(0.0);
//!
//!         for i in 0..10 {
//!             token.throw_if_cancelled()?;
//!             sleep(Duration::from_millis(100)).await;
//!             progress.update((i + 1) as f32 / 10.0, Some(format!("Step {}/10", i + 1)));
//!         }
//!
//!         Ok("Done!")
//!     });
//!
//!     let handle = executor.spawn(task);
//!     let result = handle.await_result().await;
//!     println!("Result: {:?}", result);
//! }
//! ```

mod executor;
mod handle;
mod task;

pub use executor::TaskExecutor;
pub use handle::{TaskHandle, TimeoutError};
pub use task::{
    BackgroundTask, FunctionTask, ProgressSender, ProgressUpdate, TaskId, TaskMetadata, TaskResult,
    TaskStatus,
};
