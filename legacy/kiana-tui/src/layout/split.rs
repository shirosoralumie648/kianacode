use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// 分屏方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal, // 水平分割（上下）
    Vertical,   // 垂直分割（左右）
}

/// 分屏节点 ID
pub type SplitId = usize;

/// 分屏内容的 trait
pub trait SplitContent {
    /// 渲染分屏内容
    fn render(&mut self, f: &mut Frame, area: Rect, focused: bool);

    /// 处理输入（可选）
    fn handle_input(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let _ = key;
        false
    }

    /// 分屏获得焦点时调用
    fn on_focus(&mut self) {}

    /// 分屏失去焦点时调用
    fn on_blur(&mut self) {}
}

/// 分屏节点
enum SplitNode {
    /// 叶子节点：包含实际内容
    Leaf {
        id: SplitId,
        content: Box<dyn SplitContent>,
    },
    /// 分支节点：包含两个子节点
    Branch {
        id: SplitId,
        direction: SplitDirection,
        ratio: f32, // 第一个子节点的比例 (0.0 - 1.0)
        left: Box<SplitNode>,
        right: Box<SplitNode>,
    },
}

impl SplitNode {
    fn id(&self) -> SplitId {
        match self {
            SplitNode::Leaf { id, .. } => *id,
            SplitNode::Branch { id, .. } => *id,
        }
    }

    /// 收集所有叶子节点 ID
    fn collect_leaves(&self, leaves: &mut Vec<SplitId>) {
        match self {
            SplitNode::Leaf { id, .. } => leaves.push(*id),
            SplitNode::Branch { left, right, .. } => {
                left.collect_leaves(leaves);
                right.collect_leaves(leaves);
            }
        }
    }
}

/// 分屏管理器
pub struct SplitManager {
    root: Option<Box<SplitNode>>,
    focused: Option<SplitId>,
    next_id: SplitId,
    min_width: u16,
    min_height: u16,
}

impl SplitManager {
    pub fn new() -> Self {
        Self {
            root: None,
            focused: None,
            next_id: 0,
            min_width: 10,
            min_height: 3,
        }
    }

    pub fn with_min_size(mut self, width: u16, height: u16) -> Self {
        self.min_width = width;
        self.min_height = height;
        self
    }

    /// 添加第一个分屏
    pub fn set_root(&mut self, content: Box<dyn SplitContent>) -> SplitId {
        let id = self.next_id;
        self.next_id += 1;

        self.root = Some(Box::new(SplitNode::Leaf { id, content }));
        self.focused = Some(id);

        id
    }

    /// 分割当前焦点分屏
    pub fn split(
        &mut self,
        direction: SplitDirection,
        content: Box<dyn SplitContent>,
    ) -> Option<SplitId> {
        let focused_id = self.focused?;
        let new_id = self.next_id;
        self.next_id += 1;

        let root = self.root.as_mut()?;

        // 如果根节点就是目标节点
        if root.id() == focused_id {
            let old_root = std::mem::replace(
                root.as_mut(),
                SplitNode::Leaf {
                    id: new_id,
                    content,
                },
            );

            let branch_id = self.next_id;
            self.next_id += 1;

            let new_leaf = Box::new(SplitNode::Leaf {
                id: new_id,
                content: match std::mem::replace(
                    root.as_mut(),
                    SplitNode::Leaf {
                        id: 0,
                        content: Box::new(DummyContent),
                    },
                ) {
                    SplitNode::Leaf { content, .. } => content,
                    _ => unreachable!(),
                },
            });

            *root = Box::new(SplitNode::Branch {
                id: branch_id,
                direction,
                ratio: 0.5,
                left: Box::new(old_root),
                right: new_leaf,
            });

            self.focused = Some(new_id);
            return Some(new_id);
        }

        // 否则查找并替换目标节点
        let branch_id = self.next_id;
        self.next_id += 1;

        if Self::split_node_recursive_static(
            root, focused_id, direction, new_id, content, branch_id,
        ) {
            self.focused = Some(new_id);
            Some(new_id)
        } else {
            None
        }
    }

    fn split_node_recursive_static(
        node: &mut Box<SplitNode>,
        target_id: SplitId,
        direction: SplitDirection,
        new_id: SplitId,
        content: Box<dyn SplitContent>,
        branch_id: SplitId,
    ) -> bool {
        match node.as_mut() {
            SplitNode::Leaf { id, .. } if *id == target_id => {
                // 找到目标节点，替换为分支节点
                let old_node = std::mem::replace(
                    node.as_mut(),
                    SplitNode::Leaf {
                        id: 0,
                        content: Box::new(DummyContent),
                    },
                );

                *node = Box::new(SplitNode::Branch {
                    id: branch_id,
                    direction,
                    ratio: 0.5,
                    left: Box::new(old_node),
                    right: Box::new(SplitNode::Leaf {
                        id: new_id,
                        content,
                    }),
                });

                true
            }
            SplitNode::Leaf { .. } => false,
            SplitNode::Branch { left, right, .. } => {
                // 首先检查左子树是否包含目标
                if Self::contains_id(left, target_id) {
                    Self::split_node_recursive_static(
                        left, target_id, direction, new_id, content, branch_id,
                    )
                } else {
                    Self::split_node_recursive_static(
                        right, target_id, direction, new_id, content, branch_id,
                    )
                }
            }
        }
    }

    // 辅助函数：检查节点是否包含指定 ID
    fn contains_id(node: &Box<SplitNode>, target_id: SplitId) -> bool {
        match node.as_ref() {
            SplitNode::Leaf { id, .. } => *id == target_id,
            SplitNode::Branch { left, right, .. } => {
                Self::contains_id(left, target_id) || Self::contains_id(right, target_id)
            }
        }
    }

    /// 关闭当前焦点分屏
    pub fn close_focused(&mut self) -> bool {
        let focused_id = match self.focused {
            Some(id) => id,
            None => return false,
        };

        // 如果只有一个分屏，不能关闭
        let mut leaves = Vec::new();
        if let Some(root) = &self.root {
            root.collect_leaves(&mut leaves);
        }

        if leaves.len() <= 1 {
            return false;
        }

        // 执行关闭
        let result = self.close_split_internal(focused_id);

        if result {
            // 重新收集叶子节点并聚焦到第一个
            leaves.clear();
            if let Some(root) = &self.root {
                root.collect_leaves(&mut leaves);
            }
            self.focused = leaves.first().copied();
        }

        result
    }

    fn close_split_internal(&mut self, target_id: SplitId) -> bool {
        let root = match self.root.as_mut() {
            Some(r) => r,
            None => return false,
        };

        // 如果根节点是目标节点，无法关闭（应该已经被上层检查拦截）
        if root.id() == target_id {
            return false;
        }

        // 查找父节点
        Self::close_split_recursive_static(root, target_id)
    }

    fn close_split_recursive_static(node: &mut Box<SplitNode>, target_id: SplitId) -> bool {
        match node.as_mut() {
            SplitNode::Leaf { .. } => false,
            SplitNode::Branch { left, right, .. } => {
                // 检查直接子节点
                if left.id() == target_id {
                    // 用右子节点替换当前分支节点
                    let right_node = std::mem::replace(
                        right.as_mut(),
                        SplitNode::Leaf {
                            id: 0,
                            content: Box::new(DummyContent),
                        },
                    );
                    *node.as_mut() = right_node;
                    return true;
                }

                if right.id() == target_id {
                    // 用左子节点替换当前分支节点
                    let left_node = std::mem::replace(
                        left.as_mut(),
                        SplitNode::Leaf {
                            id: 0,
                            content: Box::new(DummyContent),
                        },
                    );
                    *node.as_mut() = left_node;
                    return true;
                }

                // 递归查找
                Self::close_split_recursive_static(left, target_id)
                    || Self::close_split_recursive_static(right, target_id)
            }
        }
    }

    /// 调整分屏大小
    pub fn resize(&mut self, delta: f32) -> bool {
        let focused_id = match self.focused {
            Some(id) => id,
            None => return false,
        };

        let root = match self.root.as_mut() {
            Some(r) => r,
            None => return false,
        };

        Self::resize_recursive_static(root, focused_id, delta)
    }

    fn resize_recursive_static(node: &mut Box<SplitNode>, target_id: SplitId, delta: f32) -> bool {
        match node.as_mut() {
            SplitNode::Leaf { .. } => false,
            SplitNode::Branch {
                left, right, ratio, ..
            } => {
                // 检查直接子节点
                if left.id() == target_id || right.id() == target_id {
                    // 调整比例
                    let new_ratio = (*ratio + delta).clamp(0.1, 0.9);
                    *ratio = new_ratio;
                    return true;
                }

                // 递归查找
                Self::resize_recursive_static(left, target_id, delta)
                    || Self::resize_recursive_static(right, target_id, delta)
            }
        }
    }

    /// 移动焦点到下一个分屏
    pub fn focus_next(&mut self) -> bool {
        let mut leaves = Vec::new();
        if let Some(root) = &self.root {
            root.collect_leaves(&mut leaves);
        }

        if leaves.is_empty() {
            return false;
        }

        let current_idx = leaves
            .iter()
            .position(|&id| Some(id) == self.focused)
            .unwrap_or(0);

        let next_idx = (current_idx + 1) % leaves.len();
        self.focused = Some(leaves[next_idx]);

        true
    }

    /// 移动焦点到上一个分屏
    pub fn focus_previous(&mut self) -> bool {
        let mut leaves = Vec::new();
        if let Some(root) = &self.root {
            root.collect_leaves(&mut leaves);
        }

        if leaves.is_empty() {
            return false;
        }

        let current_idx = leaves
            .iter()
            .position(|&id| Some(id) == self.focused)
            .unwrap_or(0);

        let prev_idx = if current_idx == 0 {
            leaves.len() - 1
        } else {
            current_idx - 1
        };

        self.focused = Some(leaves[prev_idx]);

        true
    }

    /// 获取焦点分屏的可变引用
    pub fn focused_content_mut(&mut self) -> Option<&mut Box<dyn SplitContent>> {
        let focused_id = self.focused?;

        if let Some(root) = &mut self.root {
            Self::get_content_mut(root, focused_id)
        } else {
            None
        }
    }

    fn get_content_mut(
        node: &mut Box<SplitNode>,
        target_id: SplitId,
    ) -> Option<&mut Box<dyn SplitContent>> {
        match node.as_mut() {
            SplitNode::Leaf { id, content } if *id == target_id => Some(content),
            SplitNode::Leaf { .. } => None,
            SplitNode::Branch { left, right, .. } => Self::get_content_mut(left, target_id)
                .or_else(|| Self::get_content_mut(right, target_id)),
        }
    }

    /// 获取分屏数量
    pub fn split_count(&self) -> usize {
        let mut leaves = Vec::new();
        if let Some(root) = &self.root {
            root.collect_leaves(&mut leaves);
        }
        leaves.len()
    }

    /// 获取焦点分屏 ID
    pub fn focused_id(&self) -> Option<SplitId> {
        self.focused
    }

    /// 渲染分屏
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let focused = self.focused;
        if let Some(root) = &mut self.root {
            render_node_helper(f, root, area, focused);
        }
    }
}

impl Default for SplitManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 辅助函数：递归渲染节点
fn render_node_helper(
    f: &mut Frame,
    node: &mut Box<SplitNode>,
    area: Rect,
    focused: Option<SplitId>,
) {
    match node.as_mut() {
        SplitNode::Leaf { id, content } => {
            let is_focused = Some(*id) == focused;

            // 渲染边框
            let border_style = if is_focused {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(border_style);

            let inner = block.inner(area);
            f.render_widget(block, area);

            // 渲染内容
            content.render(f, inner, is_focused);
        }
        SplitNode::Branch {
            direction,
            ratio,
            left,
            right,
            ..
        } => {
            // 计算分割区域
            let ratio_val = *ratio;
            let (dir, constraints) = match direction {
                SplitDirection::Horizontal => (
                    Direction::Vertical,
                    vec![
                        Constraint::Percentage((ratio_val * 100.0) as u16),
                        Constraint::Percentage(((1.0 - ratio_val) * 100.0) as u16),
                    ],
                ),
                SplitDirection::Vertical => (
                    Direction::Horizontal,
                    vec![
                        Constraint::Percentage((ratio_val * 100.0) as u16),
                        Constraint::Percentage(((1.0 - ratio_val) * 100.0) as u16),
                    ],
                ),
            };

            let chunks = Layout::default()
                .direction(dir)
                .constraints(constraints)
                .split(area);

            // 递归渲染子节点
            if chunks.len() >= 2 {
                render_node_helper(f, left, chunks[0], focused);
                render_node_helper(f, right, chunks[1], focused);
            }
        }
    }
}

/// 临时占位内容（用于节点替换过程中）
struct DummyContent;

impl SplitContent for DummyContent {
    fn render(&mut self, f: &mut Frame, area: Rect, _focused: bool) {
        let text = Line::from("Dummy Content");
        let paragraph = Paragraph::new(text);
        f.render_widget(paragraph, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // Mock SplitContent for testing
    struct MockSplitContent {
        name: String,
        render_count: Arc<Mutex<usize>>,
        focus_count: Arc<Mutex<usize>>,
        blur_count: Arc<Mutex<usize>>,
    }

    impl MockSplitContent {
        fn new(
            name: &str,
        ) -> (
            Self,
            Arc<Mutex<usize>>,
            Arc<Mutex<usize>>,
            Arc<Mutex<usize>>,
        ) {
            let render_count = Arc::new(Mutex::new(0));
            let focus_count = Arc::new(Mutex::new(0));
            let blur_count = Arc::new(Mutex::new(0));

            (
                Self {
                    name: name.to_string(),
                    render_count: render_count.clone(),
                    focus_count: focus_count.clone(),
                    blur_count: blur_count.clone(),
                },
                render_count,
                focus_count,
                blur_count,
            )
        }
    }

    impl SplitContent for MockSplitContent {
        fn render(&mut self, _f: &mut Frame, _area: Rect, _focused: bool) {
            *self.render_count.lock().unwrap() += 1;
        }

        fn on_focus(&mut self) {
            *self.focus_count.lock().unwrap() += 1;
        }

        fn on_blur(&mut self) {
            *self.blur_count.lock().unwrap() += 1;
        }
    }

    #[test]
    fn test_split_manager_creation() {
        let manager = SplitManager::new();
        assert_eq!(manager.split_count(), 0);
        assert_eq!(manager.focused_id(), None);
    }

    #[test]
    fn test_set_root() {
        let mut manager = SplitManager::new();
        let (content, _, _, _) = MockSplitContent::new("root");

        let id = manager.set_root(Box::new(content));

        assert_eq!(manager.split_count(), 1);
        assert_eq!(manager.focused_id(), Some(id));
    }

    #[test]
    fn test_split_horizontal() {
        let mut manager = SplitManager::new();
        let (content1, _, _, _) = MockSplitContent::new("split1");
        let (content2, _, _, _) = MockSplitContent::new("split2");

        manager.set_root(Box::new(content1));
        let new_id = manager.split(SplitDirection::Horizontal, Box::new(content2));

        assert!(new_id.is_some());
        assert_eq!(manager.split_count(), 2);
        assert_eq!(manager.focused_id(), new_id);
    }

    #[test]
    fn test_split_vertical() {
        let mut manager = SplitManager::new();
        let (content1, _, _, _) = MockSplitContent::new("split1");
        let (content2, _, _, _) = MockSplitContent::new("split2");

        manager.set_root(Box::new(content1));
        let new_id = manager.split(SplitDirection::Vertical, Box::new(content2));

        assert!(new_id.is_some());
        assert_eq!(manager.split_count(), 2);
    }

    #[test]
    fn test_close_focused_single_split() {
        let mut manager = SplitManager::new();
        let (content, _, _, _) = MockSplitContent::new("root");

        manager.set_root(Box::new(content));

        // 不能关闭唯一的分屏
        assert!(!manager.close_focused());
        assert_eq!(manager.split_count(), 1);
    }

    #[test]
    fn test_close_focused_multiple_splits() {
        let mut manager = SplitManager::new();
        let (content1, _, _, _) = MockSplitContent::new("split1");
        let (content2, _, _, _) = MockSplitContent::new("split2");

        manager.set_root(Box::new(content1));
        manager.split(SplitDirection::Horizontal, Box::new(content2));

        assert_eq!(manager.split_count(), 2);

        // 关闭当前焦点分屏
        assert!(manager.close_focused());
        assert_eq!(manager.split_count(), 1);
    }

    #[test]
    fn test_nested_splits() {
        let mut manager = SplitManager::new();

        // 创建嵌套分屏结构
        let (content1, _, _, _) = MockSplitContent::new("split1");
        manager.set_root(Box::new(content1));

        let (content2, _, _, _) = MockSplitContent::new("split2");
        manager.split(SplitDirection::Horizontal, Box::new(content2));

        let (content3, _, _, _) = MockSplitContent::new("split3");
        manager.split(SplitDirection::Vertical, Box::new(content3));

        assert_eq!(manager.split_count(), 3);
    }

    #[test]
    fn test_min_size_constraints() {
        let manager = SplitManager::new().with_min_size(20, 5);

        assert_eq!(manager.min_width, 20);
        assert_eq!(manager.min_height, 5);
    }

    #[test]
    fn test_focus_next() {
        let mut manager = SplitManager::new();
        let (content1, _, _, _) = MockSplitContent::new("split1");
        let (content2, _, _, _) = MockSplitContent::new("split2");

        let id1 = manager.set_root(Box::new(content1));
        let id2 = manager.split(SplitDirection::Horizontal, Box::new(content2));

        // 当前焦点在 id2
        assert_eq!(manager.focused_id(), id2);

        // 移动到下一个
        manager.focus_next();
        assert_eq!(manager.focused_id(), Some(id1));

        // 再次移动应该循环回到 id2
        manager.focus_next();
        assert_eq!(manager.focused_id(), id2);
    }

    #[test]
    fn test_focus_previous() {
        let mut manager = SplitManager::new();
        let (content1, _, _, _) = MockSplitContent::new("split1");
        let (content2, _, _, _) = MockSplitContent::new("split2");

        let id1 = manager.set_root(Box::new(content1));
        manager.split(SplitDirection::Horizontal, Box::new(content2));

        // 移动到上一个
        manager.focus_previous();
        assert_eq!(manager.focused_id(), Some(id1));
    }

    #[test]
    fn test_resize() {
        let mut manager = SplitManager::new();
        let (content1, _, _, _) = MockSplitContent::new("split1");
        let (content2, _, _, _) = MockSplitContent::new("split2");

        manager.set_root(Box::new(content1));
        manager.split(SplitDirection::Horizontal, Box::new(content2));

        // 调整大小
        assert!(manager.resize(0.1));
        assert!(manager.resize(-0.1));
    }
}
