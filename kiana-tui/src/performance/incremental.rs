use ratatui::layout::Rect;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Widget唯一标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WidgetId(u64);

impl WidgetId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn from_path(path: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        Self(hasher.finish())
    }

    pub fn with_index(base: &str, index: usize) -> Self {
        let path = format!("{}[{}]", base, index);
        Self::from_path(&path)
    }
}

/// Widget状态快照
#[derive(Debug, Clone)]
pub struct WidgetState {
    pub id: WidgetId,
    pub content_hash: u64,
    pub bounds: Rect,
    pub children: Vec<WidgetId>,
    pub z_index: i32,
}

impl WidgetState {
    pub fn new(id: WidgetId, bounds: Rect) -> Self {
        Self {
            id,
            content_hash: 0,
            bounds,
            children: Vec::new(),
            z_index: 0,
        }
    }

    pub fn with_content_hash(mut self, hash: u64) -> Self {
        self.content_hash = hash;
        self
    }

    pub fn with_children(mut self, children: Vec<WidgetId>) -> Self {
        self.children = children;
        self
    }

    pub fn with_z_index(mut self, z_index: i32) -> Self {
        self.z_index = z_index;
        self
    }
}

/// Widget树
#[derive(Debug, Clone, Default)]
pub struct WidgetTree {
    nodes: HashMap<WidgetId, WidgetState>,
    root: Option<WidgetId>,
}

impl WidgetTree {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            root: None,
        }
    }

    pub fn insert(&mut self, state: WidgetState) {
        if self.root.is_none() {
            self.root = Some(state.id);
        }
        self.nodes.insert(state.id, state);
    }

    pub fn get(&self, id: &WidgetId) -> Option<&WidgetState> {
        self.nodes.get(id)
    }

    pub fn remove(&mut self, id: &WidgetId) -> Option<WidgetState> {
        self.nodes.remove(id)
    }

    pub fn root(&self) -> Option<WidgetId> {
        self.root
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }
}

/// 变更类型
#[derive(Debug, Clone)]
pub enum Change {
    /// 插入新widget
    Insert {
        parent: WidgetId,
        state: WidgetState,
        index: usize,
    },
    /// 移除widget
    Remove { id: WidgetId },
    /// 更新widget内容
    Update {
        id: WidgetId,
        new_state: WidgetState,
    },
    /// 移动widget位置
    Move {
        id: WidgetId,
        new_parent: WidgetId,
        new_index: usize,
    },
    /// 重新排序子节点
    Reorder {
        parent: WidgetId,
        new_order: Vec<WidgetId>,
    },
}

/// 差分引擎 - 计算widget树的变化
pub struct DiffEngine {
    previous_tree: WidgetTree,
    changes: Vec<Change>,
}

impl DiffEngine {
    pub fn new() -> Self {
        Self {
            previous_tree: WidgetTree::new(),
            changes: Vec::new(),
        }
    }

    /// 计算两个树之间的差异
    pub fn diff(&mut self, current_tree: &WidgetTree) -> &[Change] {
        self.changes.clear();

        // 如果没有前一个树，所有节点都是新增的
        if self.previous_tree.is_empty() {
            self.diff_initial(current_tree);
            self.previous_tree = current_tree.clone();
            return &self.changes;
        }

        // 深度优先遍历比较
        if let Some(root_id) = current_tree.root() {
            self.diff_subtree(root_id, current_tree);
        }

        // 查找被删除的节点
        self.find_removed_nodes(current_tree);

        // 更新缓存的树
        self.previous_tree = current_tree.clone();

        &self.changes
    }

    /// 初始差分 - 第一次渲染
    fn diff_initial(&mut self, tree: &WidgetTree) {
        for (id, state) in &tree.nodes {
            self.changes.push(Change::Insert {
                parent: *id, // 临时使用自己作为父节点
                state: state.clone(),
                index: 0,
            });
        }
    }

    /// 递归比较子树
    fn diff_subtree(&mut self, id: WidgetId, current_tree: &WidgetTree) {
        let current_state = match current_tree.get(&id) {
            Some(s) => s,
            None => return,
        };

        let previous_state = self.previous_tree.get(&id);

        match previous_state {
            None => {
                // 新节点
                self.changes.push(Change::Insert {
                    parent: id,
                    state: current_state.clone(),
                    index: 0,
                });
            }
            Some(prev) => {
                // 检查内容是否变化
                if current_state.content_hash != prev.content_hash
                    || current_state.bounds != prev.bounds
                    || current_state.z_index != prev.z_index
                {
                    self.changes.push(Change::Update {
                        id,
                        new_state: current_state.clone(),
                    });
                }

                // 检查子节点变化 - 克隆以避免借用冲突
                let prev_children = prev.children.clone();
                let current_children = current_state.children.clone();
                if current_children != prev_children {
                    self.diff_children(id, &prev_children, &current_children, current_tree);
                }
            }
        }

        // 递归处理子节点
        let children = current_state.children.clone();
        for child_id in &children {
            self.diff_subtree(*child_id, current_tree);
        }
    }

    /// 比较子节点列表
    fn diff_children(
        &mut self,
        parent: WidgetId,
        old_children: &[WidgetId],
        new_children: &[WidgetId],
        current_tree: &WidgetTree,
    ) {
        // 简单策略：检测新增、删除和重排序
        let old_set: std::collections::HashSet<_> = old_children.iter().collect();
        let new_set: std::collections::HashSet<_> = new_children.iter().collect();

        // 查找新增的子节点
        for (index, &child_id) in new_children.iter().enumerate() {
            if !old_set.contains(&child_id) {
                if let Some(state) = current_tree.get(&child_id) {
                    self.changes.push(Change::Insert {
                        parent,
                        state: state.clone(),
                        index,
                    });
                }
            }
        }

        // 查找删除的子节点
        for &child_id in old_children {
            if !new_set.contains(&child_id) {
                self.changes.push(Change::Remove { id: child_id });
            }
        }

        // 检测重排序
        if old_children.len() == new_children.len() && old_set == new_set {
            if old_children != new_children {
                self.changes.push(Change::Reorder {
                    parent,
                    new_order: new_children.to_vec(),
                });
            }
        }
    }

    /// 查找被删除的节点
    fn find_removed_nodes(&mut self, current_tree: &WidgetTree) {
        for id in self.previous_tree.nodes.keys() {
            if !current_tree.nodes.contains_key(id) {
                self.changes.push(Change::Remove { id: *id });
            }
        }
    }

    /// 应用变更到树
    pub fn apply_changes(&self, tree: &mut WidgetTree, changes: &[Change]) {
        for change in changes {
            match change {
                Change::Insert { state, .. } => {
                    tree.insert(state.clone());
                }
                Change::Remove { id } => {
                    tree.remove(id);
                }
                Change::Update { id: _, new_state } => {
                    tree.insert(new_state.clone());
                }
                Change::Move { id, new_parent, .. } => {
                    if let Some(state) = tree.remove(id) {
                        // 更新父节点引用
                        if let Some(parent_state) = tree.nodes.get_mut(new_parent) {
                            if !parent_state.children.contains(id) {
                                parent_state.children.push(*id);
                            }
                        }
                        tree.insert(state);
                    }
                }
                Change::Reorder { parent, new_order } => {
                    if let Some(parent_state) = tree.nodes.get_mut(parent) {
                        parent_state.children = new_order.clone();
                    }
                }
            }
        }
    }

    /// 获取变更数量
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }

    /// 重置状态
    pub fn reset(&mut self) {
        self.previous_tree.clear();
        self.changes.clear();
    }
}

impl Default for DiffEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// 可哈希的Widget trait
pub trait Hashable {
    fn content_hash(&self) -> u64;
}

/// 辅助函数：计算字符串哈希
pub fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// 辅助函数：计算多个值的组合哈希
pub fn hash_values<T: Hash>(values: &[T]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for value in values {
        value.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_id_generation() {
        let id1 = WidgetId::from_path("root.panel.button");
        let id2 = WidgetId::from_path("root.panel.button");
        let id3 = WidgetId::from_path("root.panel.text");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_widget_tree_basic() {
        let mut tree = WidgetTree::new();
        let id = WidgetId::new(1);
        let state = WidgetState::new(id, Rect::default());

        tree.insert(state.clone());
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.root(), Some(id));

        let retrieved = tree.get(&id).unwrap();
        assert_eq!(retrieved.id, id);
    }

    #[test]
    fn test_diff_engine_insert() {
        let mut engine = DiffEngine::new();
        let mut tree = WidgetTree::new();

        let id = WidgetId::new(1);
        tree.insert(WidgetState::new(id, Rect::default()));

        let changes = engine.diff(&tree);
        assert_eq!(changes.len(), 1);

        match &changes[0] {
            Change::Insert { state, .. } => assert_eq!(state.id, id),
            _ => panic!("Expected Insert change"),
        }
    }

    #[test]
    fn test_diff_engine_update() {
        let mut engine = DiffEngine::new();

        // 初始树
        let mut tree1 = WidgetTree::new();
        let id = WidgetId::new(1);
        tree1.insert(WidgetState::new(id, Rect::default()).with_content_hash(100));
        engine.diff(&tree1);

        // 更新后的树
        let mut tree2 = WidgetTree::new();
        tree2.insert(WidgetState::new(id, Rect::default()).with_content_hash(200));

        let changes = engine.diff(&tree2);
        assert_eq!(changes.len(), 1);

        match &changes[0] {
            Change::Update {
                id: changed_id,
                new_state,
            } => {
                assert_eq!(*changed_id, id);
                assert_eq!(new_state.content_hash, 200);
            }
            _ => panic!("Expected Update change"),
        }
    }

    #[test]
    fn test_diff_engine_remove() {
        let mut engine = DiffEngine::new();

        // 初始树
        let mut tree1 = WidgetTree::new();
        let id = WidgetId::new(1);
        tree1.insert(WidgetState::new(id, Rect::default()));
        engine.diff(&tree1);

        // 空树
        let tree2 = WidgetTree::new();
        let changes = engine.diff(&tree2);

        // 应该检测到删除
        assert!(changes
            .iter()
            .any(|c| matches!(c, Change::Remove { id: removed_id } if *removed_id == id)));
    }
}
