/// Layout management for the TUI application
pub mod panels;
pub mod split;
pub mod tabs;
pub mod virtual_list;

#[cfg(test)]
pub mod example_splits;

#[cfg(test)]
pub mod example_tabs;

#[cfg(test)]
mod integration_example;

pub use panels::{Panel, PanelContainer, PanelContent, PanelDirection, PanelSize};
pub use split::{SplitContent, SplitDirection, SplitId, SplitManager};
pub use tabs::{Tab, TabContainer, TabContent};
pub use virtual_list::{ItemHeight, ScrollState, VirtualList, VirtualListItem};
