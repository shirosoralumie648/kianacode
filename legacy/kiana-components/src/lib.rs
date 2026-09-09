// Core TUI component library for Kiana
// Converted from React/Ink components to ratatui

pub mod async_task;
pub mod dialog;
pub mod layout;
pub mod list;
pub mod log_viewer;
pub mod preview;
pub mod progress;
pub mod scrollbar;
pub mod spinner;
pub mod text;
pub mod theme;

pub use async_task::{
    AsyncTaskManager, CancellationToken, ErrorKind, ProgressReporter, TaskContext, TaskError,
    TaskHandle, TaskId, TaskMetadata, TaskPriority, TaskProgress, TaskResult, TaskStatus,
};
pub use dialog::Dialog;
pub use layout::{Box, FlexDirection};
pub use list::ListItem;
pub use log_viewer::{LogBuffer, LogEntry, LogLevel, LogParser, LogStats, LogViewer};
pub use preview::{
    BinaryPreviewer, FileType, FileTypeDetector, MarkdownPreviewer, PreviewContent, PreviewPane,
    TextPreviewer, WrapMode,
};
pub use progress::ProgressBar;
pub use scrollbar::{Scrollbar, ScrollbarOrientation, ScrollbarState};
pub use spinner::Spinner;
pub use text::Text;
pub use theme::{Theme, ThemedStyle};
