use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, StatefulWidget, Widget},
};
use std::collections::HashMap;

/// 树节点的唯一标识符
pub type NodeId = String;

/// 树节点数据
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub id: NodeId,
    pub label: String,
    pub icon: Option<String>,
    pub metadata: Option<String>,
    pub is_expandable: bool,
    pub is_expanded: bool,
    pub children_loaded: bool,
    pub parent_id: Option<NodeId>,
}

/// 树视图状态
#[derive(Clone, Debug)]
pub struct TreeViewState {
    /// 所有节点的扁平映射
    nodes: HashMap<NodeId, TreeNode>,
    /// 根节点 ID 列表
    root_ids: Vec<NodeId>,
    /// 当前选中的节点 ID
    selected_id: Option<NodeId>,
    /// 可见节点列表（展开状态下的扁平视图）
    visible_nodes: Vec<NodeId>,
    /// 滚动偏移
    scroll_offset: usize,
    /// 搜索过滤文本
    search_filter: Option<String>,
}

impl TreeViewState {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            root_ids: Vec::new(),
            selected_id: None,
            visible_nodes: Vec::new(),
            scroll_offset: 0,
            search_filter: None,
        }
    }

    /// 添加根节点
    pub fn add_root(&mut self, node: TreeNode) {
        let id = node.id.clone();
        self.nodes.insert(id.clone(), node);
        self.root_ids.push(id);
        self.rebuild_visible_list();
    }

    /// 添加子节点
    pub fn add_child(&mut self, parent_id: &NodeId, node: TreeNode) {
        let id = node.id.clone();
        let mut node = node;
        node.parent_id = Some(parent_id.clone());
        self.nodes.insert(id, node);
        if let Some(parent) = self.nodes.get_mut(parent_id) {
            parent.children_loaded = true;
        }
        self.rebuild_visible_list();
    }

    /// 展开/折叠节点
    pub fn toggle_expand(&mut self, node_id: &NodeId) -> bool {
        let should_rebuild = if let Some(node) = self.nodes.get_mut(node_id) {
            if node.is_expandable {
                node.is_expanded = !node.is_expanded;
                true
            } else {
                false
            }
        } else {
            false
        };

        if should_rebuild {
            self.rebuild_visible_list();
            if let Some(node) = self.nodes.get(node_id) {
                return node.is_expanded;
            }
        }
        false
    }

    /// 重建可见节点列表
    fn rebuild_visible_list(&mut self) {
        self.visible_nodes.clear();

        for root_id in &self.root_ids.clone() {
            self.collect_visible_nodes(root_id, 0);
        }
    }

    fn collect_visible_nodes(&mut self, node_id: &NodeId, _depth: usize) {
        // 应用搜索过滤
        if let Some(filter) = &self.search_filter {
            if let Some(node) = self.nodes.get(node_id) {
                if !node.label.to_lowercase().contains(&filter.to_lowercase()) {
                    return;
                }
            }
        }

        self.visible_nodes.push(node_id.clone());

        // 如果节点已展开，递归收集子节点
        if let Some(node) = self.nodes.get(node_id).cloned() {
            if node.is_expanded {
                let children = self.get_children(node_id);
                for child_id in children {
                    self.collect_visible_nodes(&child_id, _depth + 1);
                }
            }
        }
    }

    fn get_children(&self, parent_id: &NodeId) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|n| n.parent_id.as_ref() == Some(parent_id))
            .map(|n| n.id.clone())
            .collect()
    }

    /// 导航方法
    pub fn select_next(&mut self) {
        if self.visible_nodes.is_empty() {
            return;
        }

        if let Some(current_id) = &self.selected_id {
            if let Some(idx) = self.visible_nodes.iter().position(|id| id == current_id) {
                if idx + 1 < self.visible_nodes.len() {
                    self.selected_id = Some(self.visible_nodes[idx + 1].clone());
                }
            }
        } else {
            self.selected_id = Some(self.visible_nodes[0].clone());
        }
    }

    pub fn select_prev(&mut self) {
        if self.visible_nodes.is_empty() {
            return;
        }

        if let Some(current_id) = &self.selected_id {
            if let Some(idx) = self.visible_nodes.iter().position(|id| id == current_id) {
                if idx > 0 {
                    self.selected_id = Some(self.visible_nodes[idx - 1].clone());
                }
            }
        } else {
            self.selected_id = Some(self.visible_nodes[0].clone());
        }
    }

    pub fn select_first(&mut self) {
        if !self.visible_nodes.is_empty() {
            self.selected_id = Some(self.visible_nodes[0].clone());
        }
    }

    pub fn select_last(&mut self) {
        if !self.visible_nodes.is_empty() {
            self.selected_id = Some(self.visible_nodes[self.visible_nodes.len() - 1].clone());
        }
    }

    /// 设置搜索过滤
    pub fn set_filter(&mut self, filter: Option<String>) {
        self.search_filter = filter;
        self.rebuild_visible_list();
    }

    /// 获取选中的节点
    pub fn get_selected(&self) -> Option<&TreeNode> {
        self.selected_id.as_ref().and_then(|id| self.nodes.get(id))
    }

    /// 处理键盘输入
    pub fn handle_key(&mut self, key: KeyEvent) -> TreeViewAction {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.select_next();
                TreeViewAction::Navigate
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.select_prev();
                TreeViewAction::Navigate
            }
            KeyCode::Char('g') => {
                self.select_first();
                TreeViewAction::Navigate
            }
            KeyCode::Char('G') => {
                self.select_last();
                TreeViewAction::Navigate
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(selected_id) = self.selected_id.clone() {
                    let expanded = self.toggle_expand(&selected_id);
                    if expanded {
                        TreeViewAction::NeedLoad(selected_id)
                    } else {
                        TreeViewAction::Collapsed
                    }
                } else {
                    TreeViewAction::None
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                // 折叠当前节点或跳到父节点
                if let Some(selected_id) = self.selected_id.clone() {
                    if let Some(node) = self.nodes.get(&selected_id) {
                        if node.is_expanded {
                            self.toggle_expand(&selected_id);
                            TreeViewAction::Collapsed
                        } else if let Some(parent_id) = &node.parent_id {
                            self.selected_id = Some(parent_id.clone());
                            TreeViewAction::Navigate
                        } else {
                            TreeViewAction::None
                        }
                    } else {
                        TreeViewAction::None
                    }
                } else {
                    TreeViewAction::None
                }
            }
            KeyCode::Char('l') | KeyCode::Right => {
                // 展开当前节点
                if let Some(selected_id) = self.selected_id.clone() {
                    if let Some(node) = self.nodes.get(&selected_id) {
                        if node.is_expandable && !node.is_expanded {
                            self.toggle_expand(&selected_id);
                            TreeViewAction::NeedLoad(selected_id)
                        } else {
                            TreeViewAction::None
                        }
                    } else {
                        TreeViewAction::None
                    }
                } else {
                    TreeViewAction::None
                }
            }
            KeyCode::Char('/') => TreeViewAction::StartSearch,
            KeyCode::Esc => {
                self.set_filter(None);
                TreeViewAction::ClearSearch
            }
            _ => TreeViewAction::None,
        }
    }

    /// 触发延迟加载
    pub fn load_children<P: TreeDataProvider>(&mut self, node_id: &NodeId, provider: &mut P) {
        if let Some(node) = self.nodes.get(node_id).cloned() {
            if !node.children_loaded && node.is_expandable {
                let children = provider.load_children(node_id);
                for mut child in children {
                    child.parent_id = Some(node_id.clone());
                    self.nodes.insert(child.id.clone(), child);
                }
                if let Some(node) = self.nodes.get_mut(node_id) {
                    node.children_loaded = true;
                }
                self.rebuild_visible_list();
            }
        }
    }

    pub fn visible_nodes(&self) -> &[NodeId] {
        &self.visible_nodes
    }

    pub fn get_node(&self, id: &NodeId) -> Option<&TreeNode> {
        self.nodes.get(id)
    }
}

impl Default for TreeViewState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TreeViewAction {
    None,
    Navigate,
    NeedLoad(NodeId),
    Collapsed,
    StartSearch,
    ClearSearch,
}

/// 延迟加载回调特征
pub trait TreeDataProvider {
    /// 加载节点的子节点
    fn load_children(&mut self, node_id: &NodeId) -> Vec<TreeNode>;

    /// 检查节点是否有子节点
    fn has_children(&self, node_id: &NodeId) -> bool;
}

/// 树视图渲染器
pub struct TreeView {
    block: Option<Block<'static>>,
    highlight_style: Style,
    normal_style: Style,
}

impl TreeView {
    pub fn new() -> Self {
        Self {
            block: None,
            highlight_style: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            normal_style: Style::default(),
        }
    }

    pub fn block(mut self, block: Block<'static>) -> Self {
        self.block = Some(block);
        self
    }

    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    pub fn normal_style(mut self, style: Style) -> Self {
        self.normal_style = style;
        self
    }

    /// 生成树形指示符（├─, └─, │）
    fn render_tree_indicator(&self, depth: usize, is_last: bool, parent_chain: &[bool]) -> String {
        let mut indicator = String::new();

        // 渲染父级缩进
        for &parent_is_last in parent_chain.iter().take(depth.saturating_sub(1)) {
            if parent_is_last {
                indicator.push_str("  ");
            } else {
                indicator.push_str("│ ");
            }
        }

        // 渲染当前级别
        if depth > 0 {
            if is_last {
                indicator.push_str("└─");
            } else {
                indicator.push_str("├─");
            }
        }

        indicator
    }

    fn calculate_depth(&self, node: &TreeNode, nodes: &HashMap<NodeId, TreeNode>) -> usize {
        let mut depth = 0;
        let mut current_id = node.parent_id.as_ref();

        while let Some(parent_id) = current_id {
            depth += 1;
            current_id = nodes.get(parent_id).and_then(|n| n.parent_id.as_ref());
        }

        depth
    }

    fn is_last_sibling(&self, node_id: &NodeId, state: &TreeViewState) -> bool {
        if let Some(node) = state.get_node(node_id) {
            if let Some(parent_id) = &node.parent_id {
                let siblings: Vec<_> = state
                    .nodes
                    .values()
                    .filter(|n| n.parent_id.as_ref() == Some(parent_id))
                    .collect();
                return siblings.last().map(|n| &n.id) == Some(node_id);
            } else {
                // 根节点
                return state.root_ids.last() == Some(node_id);
            }
        }
        false
    }

    fn build_parent_chain(&self, node: &TreeNode, nodes: &HashMap<NodeId, TreeNode>) -> Vec<bool> {
        let mut chain = Vec::new();
        let mut current_id = node.parent_id.as_ref();

        while let Some(parent_id) = current_id {
            if let Some(parent) = nodes.get(parent_id) {
                chain.push(self.is_last_sibling_by_id(parent_id, parent, nodes));
                current_id = parent.parent_id.as_ref();
            } else {
                break;
            }
        }

        chain.reverse();
        chain
    }

    fn is_last_sibling_by_id(
        &self,
        node_id: &NodeId,
        node: &TreeNode,
        nodes: &HashMap<NodeId, TreeNode>,
    ) -> bool {
        if let Some(parent_id) = &node.parent_id {
            let siblings: Vec<_> = nodes
                .values()
                .filter(|n| n.parent_id.as_ref() == Some(parent_id))
                .collect();
            siblings.last().map(|n| &n.id) == Some(node_id)
        } else {
            false
        }
    }
}

impl Default for TreeView {
    fn default() -> Self {
        Self::new()
    }
}

impl StatefulWidget for TreeView {
    type State = TreeViewState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let inner = if let Some(ref block) = self.block {
            let inner = block.inner(area);
            block.clone().render(area, buf);
            inner
        } else {
            area
        };

        let viewport_height = inner.height as usize;

        // 调整滚动偏移以保持选中项可见
        if let Some(selected_id) = &state.selected_id {
            if let Some(selected_idx) = state.visible_nodes.iter().position(|id| id == selected_id)
            {
                if selected_idx < state.scroll_offset {
                    state.scroll_offset = selected_idx;
                } else if selected_idx >= state.scroll_offset + viewport_height {
                    state.scroll_offset = selected_idx.saturating_sub(viewport_height - 1);
                }
            }
        }

        // 渲染可见范围内的节点
        let end = (state.scroll_offset + viewport_height).min(state.visible_nodes.len());
        let visible_range = state.scroll_offset..end;

        for (line_idx, node_id) in state.visible_nodes[visible_range].iter().enumerate() {
            if let Some(node) = state.nodes.get(node_id) {
                let depth = self.calculate_depth(node, &state.nodes);
                let is_last = self.is_last_sibling(node_id, state);
                let parent_chain = self.build_parent_chain(node, &state.nodes);

                let indicator = self.render_tree_indicator(depth, is_last, &parent_chain);

                // 构建显示文本
                let mut spans = vec![Span::raw(indicator)];

                // 展开/折叠图标
                if node.is_expandable {
                    let icon = if node.is_expanded { "▼ " } else { "▶ " };
                    spans.push(Span::raw(icon));
                } else {
                    spans.push(Span::raw("  "));
                }

                // 节点图标
                if let Some(icon) = &node.icon {
                    spans.push(Span::raw(format!("{} ", icon)));
                }

                // 节点标签
                let label_style = if Some(node_id) == state.selected_id.as_ref() {
                    self.highlight_style
                } else {
                    self.normal_style
                };
                spans.push(Span::styled(&node.label, label_style));

                // 元数据
                if let Some(metadata) = &node.metadata {
                    spans.push(Span::styled(
                        format!(" {}", metadata),
                        Style::default().fg(Color::DarkGray),
                    ));
                }

                let line = Line::from(spans);
                let y = inner.y + line_idx as u16;
                if y < inner.y + inner.height {
                    buf.set_line(inner.x, y, &line, inner.width);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_view_state_creation() {
        let state = TreeViewState::new();
        assert!(state.visible_nodes.is_empty());
        assert!(state.selected_id.is_none());
    }

    #[test]
    fn test_add_root_node() {
        let mut state = TreeViewState::new();
        let node = TreeNode {
            id: "root1".to_string(),
            label: "Root".to_string(),
            icon: Some("📁".to_string()),
            metadata: None,
            is_expandable: true,
            is_expanded: false,
            children_loaded: false,
            parent_id: None,
        };

        state.add_root(node);
        assert_eq!(state.visible_nodes.len(), 1);
        assert_eq!(state.root_ids.len(), 1);
    }

    #[test]
    fn test_expand_collapse() {
        let mut state = TreeViewState::new();

        let root = TreeNode {
            id: "root".to_string(),
            label: "Root".to_string(),
            icon: None,
            metadata: None,
            is_expandable: true,
            is_expanded: false,
            children_loaded: false,
            parent_id: None,
        };
        state.add_root(root);

        let child = TreeNode {
            id: "child1".to_string(),
            label: "Child 1".to_string(),
            icon: None,
            metadata: None,
            is_expandable: false,
            is_expanded: false,
            children_loaded: false,
            parent_id: Some("root".to_string()),
        };
        state.add_child(&"root".to_string(), child);

        // 初始状态：只有根节点可见
        assert_eq!(state.visible_nodes.len(), 1);

        // 展开根节点
        state.toggle_expand(&"root".to_string());
        assert_eq!(state.visible_nodes.len(), 2);

        // 折叠根节点
        state.toggle_expand(&"root".to_string());
        assert_eq!(state.visible_nodes.len(), 1);
    }

    #[test]
    fn test_navigation() {
        let mut state = TreeViewState::new();

        for i in 0..5 {
            let node = TreeNode {
                id: format!("node{}", i),
                label: format!("Node {}", i),
                icon: None,
                metadata: None,
                is_expandable: false,
                is_expanded: false,
                children_loaded: false,
                parent_id: None,
            };
            state.add_root(node);
        }

        // 选择第一个
        state.select_first();
        assert_eq!(state.selected_id, Some("node0".to_string()));

        // 下移
        state.select_next();
        assert_eq!(state.selected_id, Some("node1".to_string()));

        // 上移
        state.select_prev();
        assert_eq!(state.selected_id, Some("node0".to_string()));

        // 跳到最后
        state.select_last();
        assert_eq!(state.selected_id, Some("node4".to_string()));
    }

    #[test]
    fn test_search_filter() {
        let mut state = TreeViewState::new();

        let nodes = vec![
            ("apple", "Apple"),
            ("banana", "Banana"),
            ("apricot", "Apricot"),
        ];

        for (id, label) in nodes {
            let node = TreeNode {
                id: id.to_string(),
                label: label.to_string(),
                icon: None,
                metadata: None,
                is_expandable: false,
                is_expanded: false,
                children_loaded: false,
                parent_id: None,
            };
            state.add_root(node);
        }

        // 无过滤：3个节点
        assert_eq!(state.visible_nodes.len(), 3);

        // 过滤 "ap"：2个节点
        state.set_filter(Some("ap".to_string()));
        assert_eq!(state.visible_nodes.len(), 2);

        // 清除过滤
        state.set_filter(None);
        assert_eq!(state.visible_nodes.len(), 3);
    }

    #[test]
    fn test_tree_indicator_rendering() {
        let view = TreeView::new();

        // 根节点
        let indicator = view.render_tree_indicator(0, false, &[]);
        assert_eq!(indicator, "");

        // 第一级，最后一个
        let indicator = view.render_tree_indicator(1, true, &[]);
        assert_eq!(indicator, "└─");

        // 第一级，非最后一个
        let indicator = view.render_tree_indicator(1, false, &[]);
        assert_eq!(indicator, "├─");

        // 第二级，父节点是最后一个
        let indicator = view.render_tree_indicator(2, false, &[true]);
        assert_eq!(indicator, "  ├─");

        // 第二级，父节点不是最后一个
        let indicator = view.render_tree_indicator(2, false, &[false]);
        assert_eq!(indicator, "│ ├─");
    }

    #[test]
    fn test_key_navigation() {
        let mut state = TreeViewState::new();

        for i in 0..3 {
            let node = TreeNode {
                id: format!("node{}", i),
                label: format!("Node {}", i),
                icon: None,
                metadata: None,
                is_expandable: false,
                is_expanded: false,
                children_loaded: false,
                parent_id: None,
            };
            state.add_root(node);
        }

        state.select_first();

        // 测试向下导航
        let action = state.handle_key(KeyEvent::from(KeyCode::Char('j')));
        assert_eq!(action, TreeViewAction::Navigate);
        assert_eq!(state.selected_id, Some("node1".to_string()));

        // 测试向上导航
        let action = state.handle_key(KeyEvent::from(KeyCode::Char('k')));
        assert_eq!(action, TreeViewAction::Navigate);
        assert_eq!(state.selected_id, Some("node0".to_string()));
    }

    #[test]
    fn test_expand_collapse_with_keys() {
        let mut state = TreeViewState::new();

        let root = TreeNode {
            id: "root".to_string(),
            label: "Root".to_string(),
            icon: None,
            metadata: None,
            is_expandable: true,
            is_expanded: false,
            children_loaded: true,
            parent_id: None,
        };
        state.add_root(root);
        state.select_first();

        // 展开
        let action = state.handle_key(KeyEvent::from(KeyCode::Char('l')));
        assert_eq!(action, TreeViewAction::NeedLoad("root".to_string()));

        // 折叠
        let action = state.handle_key(KeyEvent::from(KeyCode::Char('h')));
        assert_eq!(action, TreeViewAction::Collapsed);
    }

    #[test]
    fn test_deep_tree_structure() {
        let mut state = TreeViewState::new();

        // 创建 3 层深的树
        let root = TreeNode {
            id: "root".to_string(),
            label: "Root".to_string(),
            icon: None,
            metadata: None,
            is_expandable: true,
            is_expanded: false,
            children_loaded: false,
            parent_id: None,
        };
        state.add_root(root);

        let child1 = TreeNode {
            id: "child1".to_string(),
            label: "Child 1".to_string(),
            icon: None,
            metadata: None,
            is_expandable: true,
            is_expanded: false,
            children_loaded: false,
            parent_id: Some("root".to_string()),
        };
        state.add_child(&"root".to_string(), child1);

        let grandchild = TreeNode {
            id: "grandchild1".to_string(),
            label: "Grandchild 1".to_string(),
            icon: None,
            metadata: None,
            is_expandable: false,
            is_expanded: false,
            children_loaded: false,
            parent_id: Some("child1".to_string()),
        };
        state.add_child(&"child1".to_string(), grandchild);

        // 展开所有节点
        state.toggle_expand(&"root".to_string());
        state.toggle_expand(&"child1".to_string());

        // 应该有 3 个可见节点
        assert_eq!(state.visible_nodes.len(), 3);
    }
}
