//! Error types for async task management

use std::error::Error as StdError;
use std::fmt;
use std::io;

/// Result type for task operations
pub type TaskResult<T> = Result<T, TaskError>;

/// Task error kinds
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// Task was cancelled
    Cancelled,
    /// Task timed out
    Timeout,
    /// I/O error occurred
    IO(io::ErrorKind),
    /// Network error
    Network,
    /// Parse error
    Parse,
    /// Runtime error
    Runtime,
    /// Custom error with description
    Custom(String),
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::Cancelled => write!(f, "cancelled"),
            ErrorKind::Timeout => write!(f, "timeout"),
            ErrorKind::IO(kind) => write!(f, "io: {:?}", kind),
            ErrorKind::Network => write!(f, "network"),
            ErrorKind::Parse => write!(f, "parse"),
            ErrorKind::Runtime => write!(f, "runtime"),
            ErrorKind::Custom(msg) => write!(f, "{}", msg),
        }
    }
}

/// Task error with context
#[derive(Debug)]
pub struct TaskError {
    pub kind: ErrorKind,
    pub message: String,
    source: Option<Box<dyn StdError + Send + Sync>>,
}

impl TaskError {
    /// Create a new task error
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
        }
    }

    /// Create a cancelled error
    pub fn cancelled() -> Self {
        Self::new(ErrorKind::Cancelled, "task was cancelled")
    }

    /// Create a timeout error
    pub fn timeout() -> Self {
        Self::new(ErrorKind::Timeout, "task timed out")
    }

    /// Create an I/O error
    pub fn io(err: io::Error) -> Self {
        let kind = err.kind();
        let message = err.to_string();
        Self {
            kind: ErrorKind::IO(kind),
            message,
            source: Some(Box::new(err)),
        }
    }

    /// Create a network error
    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Network, message)
    }

    /// Create a parse error
    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Parse, message)
    }

    /// Create a runtime error
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Runtime, message)
    }

    /// Create a custom error
    pub fn custom(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Custom("custom".to_string()), message)
    }

    /// Add a source error
    pub fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Get the error kind
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// Check if error is cancellation
    pub fn is_cancelled(&self) -> bool {
        matches!(self.kind, ErrorKind::Cancelled)
    }

    /// Check if error is timeout
    pub fn is_timeout(&self) -> bool {
        matches!(self.kind, ErrorKind::Timeout)
    }
}

impl fmt::Display for TaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl StdError for TaskError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn StdError + 'static))
    }
}

impl From<io::Error> for TaskError {
    fn from(err: io::Error) -> Self {
        Self::io(err)
    }
}

impl From<String> for TaskError {
    fn from(msg: String) -> Self {
        Self::custom(msg)
    }
}

impl From<&str> for TaskError {
    fn from(msg: &str) -> Self {
        Self::custom(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_kinds() {
        let err = TaskError::cancelled();
        assert!(err.is_cancelled());
        assert!(!err.is_timeout());

        let err = TaskError::timeout();
        assert!(err.is_timeout());
        assert!(!err.is_cancelled());
    }

    #[test]
    fn test_custom_error() {
        let err = TaskError::custom("something went wrong");
        assert_eq!(err.message, "something went wrong");
        assert!(matches!(err.kind, ErrorKind::Custom(_)));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let task_err = TaskError::from(io_err);
        assert!(matches!(
            task_err.kind,
            ErrorKind::IO(io::ErrorKind::NotFound)
        ));
    }

    #[test]
    fn test_string_conversion() {
        let err: TaskError = "test error".into();
        assert_eq!(err.message, "test error");

        let err: TaskError = String::from("another error").into();
        assert_eq!(err.message, "another error");
    }

    #[test]
    fn test_error_display() {
        let err = TaskError::network("connection failed");
        let display = format!("{}", err);
        assert!(display.contains("network"));
        assert!(display.contains("connection failed"));
    }
}
