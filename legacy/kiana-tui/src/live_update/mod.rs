//! Live update system for real-time data polling.
//!
//! This module provides infrastructure for periodically fetching data
//! and notifying listeners of updates.
//!
//! # Example
//!
//! ```rust,no_run
//! use kiana_tui::live_update::{LivePoller, PollConfig, FunctionSource};
//! use tokio::time::Duration;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Create a data source
//!     let source = FunctionSource::new(|| async {
//!         // Fetch some data
//!         Ok(42)
//!     });
//!
//!     // Configure polling
//!     let config = PollConfig::new(Duration::from_secs(1));
//!
//!     // Create poller
//!     let (poller, mut rx) = LivePoller::new(source, config);
//!
//!     // Start polling
//!     poller.start().await.unwrap();
//!
//!     // Receive updates
//!     while let Some(update) = rx.recv().await {
//!         println!("Update: {:?}", update);
//!     }
//! }
//! ```

mod poller;
mod source;

pub use poller::{LivePoller, PollConfig, PollerState, RetryConfig, UpdateResult};
pub use source::{ComparableSource, DataSource, FunctionSource};
