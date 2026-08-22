// Log Viewer Component
// Streaming log display with filtering, search, and color coding

mod buffer;
mod parser;
mod viewer;

pub use buffer::{LogBuffer, LogStats};
pub use parser::{LogEntry, LogLevel, LogParser};
pub use viewer::LogViewer;
