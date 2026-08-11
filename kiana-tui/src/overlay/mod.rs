// kiana-tui/src/overlay/mod.rs
pub mod search;

use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

pub use search::{SearchMode, SearchOverlay, SearchResult};

/// 覆盖层操作结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayAction {
    /// 继续显示覆盖层
    Continue,
    /// 关闭覆盖层（取消操作）
    Close,
    /// 提交结果并关闭
    Submit(String),
}

/// 通用覆盖层 trait
///
/// 覆盖层是显示在主界面之上的弹出式组件，用于处理临时交互
/// 例如：搜索历史、命令面板、帮助文档等
pub trait Overlay {
    /// 渲染覆盖层内容
    fn render(&self, frame: &mut Frame, area: Rect);

    /// 处理键盘事件
    ///
    /// 返回 OverlayAction 指示下一步操作
    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction;

    /// 获取覆盖层标题（用于显示）
    fn title(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    // 验证 OverlayAction 的基本行为
    #[test]
    fn test_overlay_action_equality() {
        assert_eq!(OverlayAction::Continue, OverlayAction::Continue);
        assert_eq!(OverlayAction::Close, OverlayAction::Close);
        assert_eq!(
            OverlayAction::Submit("test".to_string()),
            OverlayAction::Submit("test".to_string())
        );
        assert_ne!(OverlayAction::Continue, OverlayAction::Close);
    }

    #[test]
    fn test_overlay_action_clone() {
        let action = OverlayAction::Submit("data".to_string());
        let cloned = action.clone();
        assert_eq!(action, cloned);
    }
}
