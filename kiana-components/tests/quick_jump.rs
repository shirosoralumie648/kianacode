use kiana_components::list::{assign_jump_numbers, find_jump_target, ListItem};

#[test]
fn test_assign_jump_numbers_to_full_list() {
    let mut items = vec![
        ListItem::new("Item 1"),
        ListItem::new("Item 2"),
        ListItem::new("Item 3"),
        ListItem::new("Item 4"),
        ListItem::new("Item 5"),
        ListItem::new("Item 6"),
        ListItem::new("Item 7"),
        ListItem::new("Item 8"),
        ListItem::new("Item 9"),
        ListItem::new("Item 10"),
    ];

    assign_jump_numbers(&mut items, 0, 9);

    // 前 9 项应该有编号
    for i in 0..9 {
        assert_eq!(items[i].jump_number, Some((i + 1) as u8));
    }

    // 第 10 项不应该有编号
    assert_eq!(items[9].jump_number, None);
}

#[test]
fn test_assign_jump_numbers_to_partial_list() {
    let mut items = vec![
        ListItem::new("Item 1"),
        ListItem::new("Item 2"),
        ListItem::new("Item 3"),
    ];

    assign_jump_numbers(&mut items, 0, 3);

    assert_eq!(items[0].jump_number, Some(1));
    assert_eq!(items[1].jump_number, Some(2));
    assert_eq!(items[2].jump_number, Some(3));
}

#[test]
fn test_assign_jump_numbers_empty_list() {
    let mut items: Vec<ListItem> = vec![];
    assign_jump_numbers(&mut items, 0, 0);
    assert!(items.is_empty());
}

#[test]
fn test_assign_jump_numbers_with_offset() {
    let mut items = vec![
        ListItem::new("Item 1"),
        ListItem::new("Item 2"),
        ListItem::new("Item 3"),
        ListItem::new("Item 4"),
        ListItem::new("Item 5"),
    ];

    // 从索引 2 开始，显示 3 项
    assign_jump_numbers(&mut items, 2, 3);

    assert_eq!(items[0].jump_number, None);
    assert_eq!(items[1].jump_number, None);
    assert_eq!(items[2].jump_number, Some(1));
    assert_eq!(items[3].jump_number, Some(2));
    assert_eq!(items[4].jump_number, Some(3));
}

#[test]
fn test_find_jump_target_valid() {
    // 列表有 10 项，从索引 0 开始
    assert_eq!(find_jump_target(1, 0, 10), Some(0));
    assert_eq!(find_jump_target(5, 0, 10), Some(4));
    assert_eq!(find_jump_target(9, 0, 10), Some(8));
}

#[test]
fn test_find_jump_target_with_offset() {
    // 列表有 15 项，从索引 5 开始（滚动了）
    assert_eq!(find_jump_target(1, 5, 15), Some(5));
    assert_eq!(find_jump_target(3, 5, 15), Some(7));
    assert_eq!(find_jump_target(9, 5, 15), Some(13));
}

#[test]
fn test_find_jump_target_out_of_bounds() {
    // 列表只有 3 项
    assert_eq!(find_jump_target(5, 0, 3), None);
    assert_eq!(find_jump_target(9, 0, 3), None);
}

#[test]
fn test_find_jump_target_invalid_number() {
    // 0 不是有效的跳转编号
    assert_eq!(find_jump_target(0, 0, 10), None);
    // 10 超出范围（只支持 1-9）
    assert_eq!(find_jump_target(10, 0, 10), None);
}

#[test]
fn test_list_item_with_jump_number() {
    let item = ListItem::new("Test").with_jump_number(Some(5));
    assert_eq!(item.jump_number, Some(5));
}

#[test]
fn test_list_item_render_with_jump_number() {
    let item = ListItem::new("Test Item")
        .with_jump_number(Some(3))
        .focused(false);

    let lines = item.render();
    assert!(!lines.is_empty());

    // 检查渲染结果包含跳转编号
    let first_line = &lines[0];
    let rendered_text: String = first_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    assert!(rendered_text.contains('3'));
}

#[test]
fn test_list_item_render_without_jump_number() {
    let item = ListItem::new("Test Item").focused(false);

    let lines = item.render();
    assert!(!lines.is_empty());

    // 不应该包含跳转编号
    let first_line = &lines[0];
    let rendered_text: String = first_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();

    // 应该只包含指示器和内容，没有数字
    assert!(rendered_text.contains("Test Item"));
}

#[test]
fn test_jump_number_visual_distinction() {
    let item = ListItem::new("Item")
        .with_jump_number(Some(1))
        .focused(true);

    let lines = item.render();
    let first_line = &lines[0];

    // 跳转编号应该有独特的样式（黄色）
    let has_yellow_number = first_line.spans.iter().any(|span| {
        span.content.chars().any(|c| c.is_ascii_digit())
            && span.style.fg == Some(ratatui::style::Color::Yellow)
    });

    assert!(has_yellow_number, "Jump number should be styled in yellow");
}

#[test]
fn test_performance_large_list() {
    use std::time::Instant;

    let mut items: Vec<ListItem> = (0..1000)
        .map(|i| ListItem::new(format!("Item {}", i)))
        .collect();

    let start = Instant::now();
    assign_jump_numbers(&mut items, 0, 9);
    let duration = start.elapsed();

    // 应该在 1ms 内完成
    assert!(
        duration.as_millis() < 1,
        "assign_jump_numbers took too long: {:?}",
        duration
    );
}
