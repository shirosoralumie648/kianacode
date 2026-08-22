//! Live polling implementation.

use super::source::DataSource;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration, Instant};

/// Configuration for a live poller.
#[derive(Debug, Clone)]
pub struct PollConfig {
    /// How often to poll for updates.
    pub interval: Duration,
    /// Whether to poll immediately on start.
    pub immediate: bool,
    /// Optional retry configuration.
    pub retry: Option<RetryConfig>,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            immediate: true,
            retry: None,
        }
    }
}

impl PollConfig {
    /// Creates a new config with the specified interval.
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            immediate: true,
            retry: None,
        }
    }

    /// Sets whether to poll immediately on start.
    pub fn with_immediate(mut self, immediate: bool) -> Self {
        self.immediate = immediate;
        self
    }

    /// Sets the retry configuration.
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.retry = Some(retry);
        self
    }
}

/// Retry configuration for failed polls.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Initial backoff duration.
    pub initial_backoff: Duration,
    /// Maximum backoff duration.
    pub max_backoff: Duration,
    /// Backoff multiplier (e.g., 2.0 for exponential backoff).
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            multiplier: 2.0,
        }
    }
}

/// State of a live poller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollerState {
    /// Poller has not been started.
    Idle,
    /// Poller is actively running.
    Running,
    /// Poller is paused.
    Paused,
    /// Poller has been stopped.
    Stopped,
}

/// Result of a poll operation.
#[derive(Debug, Clone)]
pub enum UpdateResult<T> {
    /// Successfully fetched new data.
    Success(T),
    /// Data has not changed.
    Unchanged,
    /// Error occurred while fetching.
    Error(String),
}

/// A live poller that periodically fetches data from a source.
pub struct LivePoller<T> {
    config: Arc<RwLock<PollConfig>>,
    source: Arc<dyn DataSource<Data = T> + Send + Sync>,
    state: Arc<RwLock<PollerState>>,
    paused: Arc<AtomicBool>,
    task_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    tx: mpsc::UnboundedSender<UpdateResult<T>>,
    trigger_tx: mpsc::UnboundedSender<()>,
    trigger_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<()>>>>,
}

impl<T: Send + Clone + 'static> LivePoller<T> {
    /// Creates a new live poller.
    ///
    /// Returns the poller and a receiver for update notifications.
    pub fn new(
        source: impl DataSource<Data = T> + 'static,
        config: PollConfig,
    ) -> (Self, mpsc::UnboundedReceiver<UpdateResult<T>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (trigger_tx, trigger_rx) = mpsc::unbounded_channel();

        let poller = Self {
            config: Arc::new(RwLock::new(config)),
            source: Arc::new(source),
            state: Arc::new(RwLock::new(PollerState::Idle)),
            paused: Arc::new(AtomicBool::new(false)),
            task_handle: Arc::new(RwLock::new(None)),
            tx,
            trigger_tx,
            trigger_rx: Arc::new(Mutex::new(Some(trigger_rx))),
        };

        (poller, rx)
    }

    /// Starts the poller.
    pub async fn start(&self) -> anyhow::Result<()> {
        let mut state = self.state.write().unwrap();
        if *state != PollerState::Idle {
            anyhow::bail!("Poller is already running or stopped");
        }
        *state = PollerState::Running;
        drop(state);

        let config = self.config.clone();
        let source = self.source.clone();
        let state = self.state.clone();
        let paused = self.paused.clone();
        let tx = self.tx.clone();
        let mut trigger_rx = self
            .trigger_rx
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| anyhow::anyhow!("Poller already started"))?;

        let handle = tokio::spawn(async move {
            let mut last_data: Option<T> = None;
            let immediate = { config.read().unwrap().immediate };
            let mut next_poll = if immediate {
                Instant::now()
            } else {
                Instant::now() + poll_interval(&config)
            };

            loop {
                // Check if stopped. Read in a sync helper so the std lock
                // guard cannot be held across await.
                if poller_stopped(&state) {
                    break;
                }

                // Wait for next poll time or manual trigger
                tokio::select! {
                    _ = sleep(next_poll.saturating_duration_since(Instant::now())) => {},
                    _ = trigger_rx.recv() => {},
                }

                // Check if paused
                if paused.load(Ordering::Acquire) {
                    sleep(Duration::from_millis(100)).await;
                    continue;
                }

                // Fetch data
                match source.fetch().await {
                    Ok(data) => {
                        let changed = match &last_data {
                            Some(old) => source.has_changed(old, &data),
                            None => true,
                        };

                        if changed {
                            let _ = tx.send(UpdateResult::Success(data.clone()));
                            last_data = Some(data);
                        } else {
                            let _ = tx.send(UpdateResult::Unchanged);
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(UpdateResult::Error(e.to_string()));
                    }
                }

                // Schedule next poll
                next_poll = Instant::now() + poll_interval(&config);
            }
        });

        *self.task_handle.write().unwrap() = Some(handle);
        Ok(())
    }

    /// Stops the poller.
    pub async fn stop(&self) {
        *self.state.write().unwrap() = PollerState::Stopped;
        if let Some(handle) = self.task_handle.write().unwrap().take() {
            handle.abort();
        }
    }

    /// Pauses the poller.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
        *self.state.write().unwrap() = PollerState::Paused;
    }

    /// Resumes the poller.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
        *self.state.write().unwrap() = PollerState::Running;
    }

    /// Triggers an immediate poll.
    pub fn trigger(&self) {
        let _ = self.trigger_tx.send(());
    }

    /// Gets the current state.
    pub fn state(&self) -> PollerState {
        *self.state.read().unwrap()
    }

    /// Updates the configuration.
    pub fn update_config(&self, config: PollConfig) {
        *self.config.write().unwrap() = config;
    }
}

impl<T> Drop for LivePoller<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.task_handle.write().unwrap().take() {
            handle.abort();
        }
    }
}


fn poller_stopped(state: &Arc<RwLock<PollerState>>) -> bool {
    *state.read().unwrap() == PollerState::Stopped
}

fn poll_interval(config: &Arc<RwLock<PollConfig>>) -> Duration {
    config.read().unwrap().interval
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_update::source::FunctionSource;
    use std::sync::atomic::AtomicU32;

    #[tokio::test]
    async fn test_poll_config() {
        let config = PollConfig::new(Duration::from_millis(100))
            .with_immediate(false)
            .with_retry(RetryConfig::default());

        assert_eq!(config.interval, Duration::from_millis(100));
        assert!(!config.immediate);
        assert!(config.retry.is_some());
    }

    #[tokio::test]
    async fn test_poller_basic() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let source = FunctionSource::new(move || {
            let c = counter_clone.clone();
            async move { Ok(c.fetch_add(1, Ordering::SeqCst)) }
        });

        let config = PollConfig::new(Duration::from_millis(50));
        let (poller, mut rx) = LivePoller::new(source, config);

        poller.start().await.unwrap();
        assert_eq!(poller.state(), PollerState::Running);

        // Should get at least 2 updates
        let _update1 = rx.recv().await.unwrap();
        let _update2 = rx.recv().await.unwrap();

        poller.stop().await;
        assert_eq!(poller.state(), PollerState::Stopped);
    }

    #[tokio::test]
    async fn test_poller_pause_resume() {
        let source = FunctionSource::new(|| async { Ok(42) });
        let config = PollConfig::new(Duration::from_millis(50));
        let (poller, mut rx) = LivePoller::new(source, config);

        poller.start().await.unwrap();

        // Get first update
        let _ = rx.recv().await.unwrap();

        poller.pause();
        assert_eq!(poller.state(), PollerState::Paused);

        sleep(Duration::from_millis(150)).await;

        // Should not receive updates while paused
        assert!(rx.try_recv().is_err());

        poller.resume();
        assert_eq!(poller.state(), PollerState::Running);

        // Should receive updates again
        let _ = rx.recv().await.unwrap();

        poller.stop().await;
    }

    #[tokio::test]
    async fn test_poller_trigger() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let source = FunctionSource::new(move || {
            let c = counter_clone.clone();
            async move { Ok(c.fetch_add(1, Ordering::SeqCst)) }
        });

        let config = PollConfig::new(Duration::from_secs(10)).with_immediate(false);
        let (poller, mut rx) = LivePoller::new(source, config);

        poller.start().await.unwrap();

        // Manually trigger
        poller.trigger();
        let update = rx.recv().await.unwrap();

        match update {
            UpdateResult::Success(val) => assert_eq!(val, 0),
            _ => panic!("Expected success"),
        }

        poller.stop().await;
    }
}
