//! Priority-based task queue with dependency management.
//!
//! This module provides a task queue that:
//! - Schedules tasks based on priority (Critical > High > Normal > Low > Idle)
//! - Handles task dependencies (tasks wait for dependencies to complete)
//! - Supports task groups (tasks in same group run serially)
//! - Controls concurrency (limits number of concurrent tasks)
//! - Provides priority aging to prevent starvation
//!
//! # Example
//!
//! ```rust,no_run
//! use kiana_tui::queue::{TaskQueue, QueueConfig, Priority};
//! use kiana_tui::background::FunctionTask;
//!
//! let config = QueueConfig::new(5); // Max 5 concurrent tasks
//! let mut queue = TaskQueue::new(config);
//!
//! // Submit a high-priority task
//! let task = FunctionTask::new("important", |_token, progress| async move {
//!     progress.message("Working...");
//!     // Do work...
//!     Ok(())
//! });
//!
//! let task_id = queue.submit(task, Priority::High);
//!
//! // Check queue stats
//! let stats = queue.stats();
//! println!("Queued: {}, Running: {}", stats.total_queued, stats.running);
//! ```

mod dependency;
mod task_queue;
mod types;

pub use dependency::DependencyGraph;
pub use task_queue::TaskQueue;
pub use types::{Priority, QueueConfig, QueueStats, SubmitOptions};
