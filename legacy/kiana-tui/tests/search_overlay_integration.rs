use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn test_search_overlay_creation() {
    use kiana_tui::overlay::search::{SearchMode, SearchOverlay};
    use kiana_tui::overlay::Overlay;

    let overlay = SearchOverlay::new(SearchMode::UserInputs);
    // 验证创建成功（基本烟雾测试）
    assert_eq!(overlay.title(), "搜索用户输入 (Ctrl+R)");
    assert_eq!(overlay.mode(), SearchMode::UserInputs);
}

#[test]
fn test_search_overlay_interaction_flow() {
    use kiana_tui::overlay::search::{SearchMode, SearchOverlay};
    use kiana_tui::overlay::{Overlay, OverlayAction};

    let messages = vec!["hello world".to_string(), "goodbye world".to_string()];

    let mut overlay = SearchOverlay::new(SearchMode::UserInputs);

    // 设置查询字符串为 "hello" 并执行搜索
    overlay.update_query("hello".to_string(), &messages);

    // 验证查询已更新并有结果
    assert_eq!(overlay.query(), "hello");
    assert!(!overlay.results().is_empty(), "Should have search results");
    assert_eq!(
        overlay.results().len(),
        1,
        "Should have one result matching 'hello'"
    );

    // 验证第一个结果是 "hello world"
    assert_eq!(overlay.results()[0].text, "hello world");

    // 按 Enter 提交选中的结果
    let action = overlay.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match action {
        OverlayAction::Submit(content) => {
            assert_eq!(content, "hello world");
        }
        _ => panic!("Expected Submit action, got {:?}", action),
    }
}

#[test]
fn test_search_overlay_escape_closes() {
    use kiana_tui::overlay::search::{SearchMode, SearchOverlay};
    use kiana_tui::overlay::{Overlay, OverlayAction};

    let messages = vec!["test".to_string()];

    let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
    overlay.set_query(String::new(), &messages);

    let action = overlay.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(action, OverlayAction::Close);
}

#[test]
fn test_search_overlay_navigation() {
    use kiana_tui::overlay::search::{SearchMode, SearchOverlay};
    use kiana_tui::overlay::{Overlay, OverlayAction};

    let messages = vec![
        "world hello".to_string(),
        "hello world".to_string(),
        "hello there".to_string(),
    ];

    let mut overlay = SearchOverlay::new(SearchMode::UserInputs);
    overlay.update_query("hello".to_string(), &messages);

    // 应该有3个结果
    assert_eq!(overlay.results().len(), 3);
    assert_eq!(overlay.selected_index(), 0);

    // 测试向下导航
    let action = overlay.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(action, OverlayAction::Continue);
    assert_eq!(overlay.selected_index(), 1);

    // 再向下
    overlay.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(overlay.selected_index(), 2);

    // 测试向上导航
    overlay.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(overlay.selected_index(), 1);
}
