use crossterm::event::{KeyCode, KeyEvent};
use kiana_tui::components::{
    NodeId, TreeDataProvider, TreeNode, TreeView, TreeViewAction, TreeViewState,
};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

struct MockFileSystemProvider {
    filesystem: std::collections::HashMap<NodeId, Vec<TreeNode>>,
}

impl MockFileSystemProvider {
    fn new() -> Self {
        let mut filesystem = std::collections::HashMap::new();

        // 模拟文件系统结构
        filesystem.insert(
            "root".to_string(),
            vec![
                TreeNode {
                    id: "src".to_string(),
                    label: "src".to_string(),
                    icon: Some("📁".to_string()),
                    metadata: Some("dir".to_string()),
                    is_expandable: true,
                    is_expanded: false,
                    children_loaded: false,
                    parent_id: Some("root".to_string()),
                },
                TreeNode {
                    id: "tests".to_string(),
                    label: "tests".to_string(),
                    icon: Some("📁".to_string()),
                    metadata: Some("dir".to_string()),
                    is_expandable: true,
                    is_expanded: false,
                    children_loaded: false,
                    parent_id: Some("root".to_string()),
                },
                TreeNode {
                    id: "cargo.toml".to_string(),
                    label: "Cargo.toml".to_string(),
                    icon: Some("📄".to_string()),
                    metadata: Some("file".to_string()),
                    is_expandable: false,
                    is_expanded: false,
                    children_loaded: false,
                    parent_id: Some("root".to_string()),
                },
            ],
        );

        filesystem.insert(
            "src".to_string(),
            vec![
                TreeNode {
                    id: "main.rs".to_string(),
                    label: "main.rs".to_string(),
                    icon: Some("🦀".to_string()),
                    metadata: Some("2.5 KB".to_string()),
                    is_expandable: false,
                    is_expanded: false,
                    children_loaded: false,
                    parent_id: Some("src".to_string()),
                },
                TreeNode {
                    id: "lib.rs".to_string(),
                    label: "lib.rs".to_string(),
                    icon: Some("🦀".to_string()),
                    metadata: Some("1.8 KB".to_string()),
                    is_expandable: false,
                    is_expanded: false,
                    children_loaded: false,
                    parent_id: Some("src".to_string()),
                },
            ],
        );

        filesystem.insert(
            "tests".to_string(),
            vec![TreeNode {
                id: "integration.rs".to_string(),
                label: "integration.rs".to_string(),
                icon: Some("🦀".to_string()),
                metadata: Some("3.2 KB".to_string()),
                is_expandable: false,
                is_expanded: false,
                children_loaded: false,
                parent_id: Some("tests".to_string()),
            }],
        );

        Self { filesystem }
    }
}

impl TreeDataProvider for MockFileSystemProvider {
    fn load_children(&mut self, node_id: &NodeId) -> Vec<TreeNode> {
        self.filesystem.get(node_id).cloned().unwrap_or_default()
    }

    fn has_children(&self, node_id: &NodeId) -> bool {
        self.filesystem.contains_key(node_id)
    }
}

#[test]
fn test_tree_view_basic_rendering() {
    let mut state = TreeViewState::new();

    let root = TreeNode {
        id: "root".to_string(),
        label: "Project Root".to_string(),
        icon: Some("📦".to_string()),
        metadata: None,
        is_expandable: true,
        is_expanded: false,
        children_loaded: false,
        parent_id: None,
    };
    state.add_root(root);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let area = f.area();
            let widget = TreeView::new();
            f.render_stateful_widget(widget, area, &mut state);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();

    // 验证根节点被渲染
    let content = buffer_to_string(buffer);
    assert!(content.contains("Project Root"));
}

#[test]
fn test_tree_view_expand_collapse_flow() {
    let mut state = TreeViewState::new();
    let mut provider = MockFileSystemProvider::new();

    let root = TreeNode {
        id: "root".to_string(),
        label: "root".to_string(),
        icon: Some("📦".to_string()),
        metadata: None,
        is_expandable: true,
        is_expanded: false,
        children_loaded: false,
        parent_id: None,
    };
    state.add_root(root);
    state.select_first();

    // 初始状态：1 个可见节点
    assert_eq!(state.visible_nodes().len(), 1);

    // 展开根节点
    let action = state.handle_key(KeyEvent::from(KeyCode::Char('l')));
    if let TreeViewAction::NeedLoad(node_id) = action {
        state.load_children(&node_id, &mut provider);
    }

    // 展开后：4 个可见节点（root + 3 children）
    assert_eq!(state.visible_nodes().len(), 4);

    // 折叠根节点
    state.handle_key(KeyEvent::from(KeyCode::Char('h')));

    // 折叠后：1 个可见节点
    assert_eq!(state.visible_nodes().len(), 1);
}

#[test]
fn test_tree_view_navigation_keys() {
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

    state.select_first();
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"node0".to_string())
    );

    // j - 向下
    state.handle_key(KeyEvent::from(KeyCode::Char('j')));
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"node1".to_string())
    );

    // k - 向上
    state.handle_key(KeyEvent::from(KeyCode::Char('k')));
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"node0".to_string())
    );

    // Down arrow
    state.handle_key(KeyEvent::from(KeyCode::Down));
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"node1".to_string())
    );

    // Up arrow
    state.handle_key(KeyEvent::from(KeyCode::Up));
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"node0".to_string())
    );

    // G - 跳到最后
    state.handle_key(KeyEvent::from(KeyCode::Char('G')));
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"node4".to_string())
    );

    // g - 跳到第一个
    state.handle_key(KeyEvent::from(KeyCode::Char('g')));
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"node0".to_string())
    );
}

#[test]
fn test_tree_view_search_filter() {
    let mut state = TreeViewState::new();

    let items = vec![
        ("rust", "Rust Language"),
        ("python", "Python Language"),
        ("ruby", "Ruby Language"),
        ("rust-analyzer", "Rust Analyzer"),
    ];

    for (id, label) in items {
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

    // 无过滤：4 个节点
    assert_eq!(state.visible_nodes().len(), 4);

    // 过滤 "rust"
    state.set_filter(Some("rust".to_string()));
    assert_eq!(state.visible_nodes().len(), 2);

    // 过滤 "python"
    state.set_filter(Some("python".to_string()));
    assert_eq!(state.visible_nodes().len(), 1);

    // 清除过滤
    state.set_filter(None);
    assert_eq!(state.visible_nodes().len(), 4);
}

#[test]
fn test_tree_view_deep_nesting() {
    let mut state = TreeViewState::new();

    // 创建 5 层深的树
    let mut parent_id = None;
    for depth in 0..5 {
        let node = TreeNode {
            id: format!("node{}", depth),
            label: format!("Level {}", depth),
            icon: None,
            metadata: None,
            is_expandable: depth < 4,
            is_expanded: false,
            children_loaded: false,
            parent_id: parent_id.clone(),
        };

        if depth == 0 {
            state.add_root(node);
        } else {
            state.add_child(&parent_id.clone().unwrap(), node);
        }

        parent_id = Some(format!("node{}", depth));
    }

    // 初始：只有根节点
    assert_eq!(state.visible_nodes().len(), 1);

    // 展开所有节点
    for depth in 0..4 {
        state.toggle_expand(&format!("node{}", depth));
    }

    // 全部展开后：5 个节点
    assert_eq!(state.visible_nodes().len(), 5);
}

#[test]
fn test_tree_view_lazy_loading() {
    let mut state = TreeViewState::new();
    let mut provider = MockFileSystemProvider::new();

    let root = TreeNode {
        id: "root".to_string(),
        label: "root".to_string(),
        icon: None,
        metadata: None,
        is_expandable: true,
        is_expanded: false,
        children_loaded: false,
        parent_id: None,
    };
    state.add_root(root);

    // 展开根节点（触发延迟加载）
    state.toggle_expand(&"root".to_string());
    state.load_children(&"root".to_string(), &mut provider);

    // 根节点的子节点已加载
    assert_eq!(state.visible_nodes().len(), 4); // root + 3 children

    // 展开 src 目录（触发第二级延迟加载）
    state.toggle_expand(&"src".to_string());
    state.load_children(&"src".to_string(), &mut provider);

    // src 的子节点已加载
    assert_eq!(state.visible_nodes().len(), 6); // root + src + 2 files + tests + cargo.toml
}

#[test]
fn test_tree_view_parent_navigation() {
    let mut state = TreeViewState::new();

    let root = TreeNode {
        id: "root".to_string(),
        label: "root".to_string(),
        icon: None,
        metadata: None,
        is_expandable: true,
        is_expanded: true,
        children_loaded: true,
        parent_id: None,
    };
    state.add_root(root);

    let child = TreeNode {
        id: "child".to_string(),
        label: "child".to_string(),
        icon: None,
        metadata: None,
        is_expandable: false,
        is_expanded: false,
        children_loaded: false,
        parent_id: Some("root".to_string()),
    };
    state.add_child(&"root".to_string(), child);

    state.select_first();
    state.select_next(); // 选中 child

    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"child".to_string())
    );

    // h - 跳到父节点
    state.handle_key(KeyEvent::from(KeyCode::Char('h')));
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"root".to_string())
    );
}

#[test]
fn test_tree_view_large_tree_performance() {
    let mut state = TreeViewState::new();

    // 创建 1000 个根节点
    for i in 0..1000 {
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

    // 验证所有节点可见
    assert_eq!(state.visible_nodes().len(), 1000);

    // 导航操作应该快速完成
    state.select_first();
    state.select_last();
    state.select_next();
    state.select_prev();

    // 搜索操作
    state.set_filter(Some("Node 1".to_string()));
    assert!(state.visible_nodes().len() > 0);
}

#[test]
fn test_tree_view_empty_tree() {
    let state = TreeViewState::new();

    // 空树
    assert_eq!(state.visible_nodes().len(), 0);
    assert!(state.get_selected().is_none());
}

#[test]
fn test_tree_view_single_node() {
    let mut state = TreeViewState::new();

    let node = TreeNode {
        id: "single".to_string(),
        label: "Single Node".to_string(),
        icon: None,
        metadata: None,
        is_expandable: false,
        is_expanded: false,
        children_loaded: false,
        parent_id: None,
    };
    state.add_root(node);

    state.select_first();

    // 向上移动应该保持在当前节点
    state.handle_key(KeyEvent::from(KeyCode::Char('k')));
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"single".to_string())
    );

    // 向下移动也应该保持在当前节点
    state.handle_key(KeyEvent::from(KeyCode::Char('j')));
    assert_eq!(
        state.get_selected().map(|n| &n.id),
        Some(&"single".to_string())
    );
}

// 辅助函数：将 buffer 转换为字符串以便断言
fn buffer_to_string(buffer: &Buffer) -> String {
    let mut result = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            result.push_str(cell.symbol());
        }
        result.push('\n');
    }
    result
}
