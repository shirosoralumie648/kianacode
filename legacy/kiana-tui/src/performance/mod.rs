// Performance optimization modules

pub mod cache;
pub mod dirty_region;
pub mod error_boundary;
pub mod incremental;
pub mod mouse;

// Re-exports for convenience
pub use cache::{CacheEntry, Cacheable, LruCache};
pub use dirty_region::{DirtyRegion, RenderCache};
pub use error_boundary::{ErrorBoundary, ErrorInfo, RecoveryStrategy};
pub use incremental::{Change, DiffEngine, WidgetId};
pub use mouse::{MouseConfig, MouseEvent, MouseHandler};
