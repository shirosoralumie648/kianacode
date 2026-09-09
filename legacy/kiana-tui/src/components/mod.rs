/// UI components for the TUI application
pub mod breadcrumbs;
pub mod charts;
pub mod command_palette;
pub mod dialog;
pub mod diff_view;
pub mod filter;
pub mod folding;
pub mod help;
pub mod macro_editor;
pub mod notifications;
pub mod progress;
pub mod spinner;
pub mod status_bar;
pub mod table;
pub mod tree_view;
pub mod vim_navigation;

pub use breadcrumbs::{BreadcrumbSegment, Breadcrumbs};
pub use charts::{
    BarChart, BarOrientation, ChartStyle, DataPoint, Interpolation, LineChart, ScalingMode,
    Sparkline, SparklineStyle,
};
pub use command_palette::{Command, CommandPalette, CommandRegistry};
pub use dialog::ConfirmDialog;
pub use diff_view::{compute_diff, DiffHunk, DiffLine, DiffMode, DiffView, DiffViewState};
pub use filter::{FilterAction, FilterInput};
pub use folding::{
    FoldIndicator, FoldKind, FoldRange, FoldRegionDetector, FoldState, FoldStateStore,
    FoldingManager, GenericFoldDetector, RustFoldDetector,
};
pub use help::HelpOverlay;
pub use macro_editor::{MacroEditor, MacroEditorAction};
pub use progress::{ProgressBar, ProgressState, ProgressStyle};
pub use spinner::{Spinner, SpinnerStyle};
pub use status_bar::{AppMode, HintCategory, HintPriority, KeyHint, StatusBar};
pub use table::{Alignment, Column, SortDirection, TableRow, TableView};
pub use tree_view::{NodeId, TreeDataProvider, TreeNode, TreeView, TreeViewAction, TreeViewState};
pub use vim_navigation::{vim_mode_to_app_mode, VimAction, VimMode, VimNavigationHandler};
