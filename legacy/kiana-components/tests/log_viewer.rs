use kiana_components::log_viewer::{LogLevel, LogParser, LogViewer};

#[test]
fn test_log_parser_basic() {
    let entry = LogParser::parse("INFO: Hello world");
    assert_eq!(entry.level, Some(LogLevel::Info));
    assert!(entry.message.contains("Hello world"));
}

#[test]
fn test_log_parser_various_levels() {
    let cases = vec![
        ("ERROR: Something failed", LogLevel::Error),
        ("WARN: Be careful", LogLevel::Warn),
        ("DEBUG: Debug info", LogLevel::Debug),
        ("TRACE: Trace message", LogLevel::Trace),
        ("FATAL: Critical error", LogLevel::Fatal),
    ];

    for (input, expected_level) in cases {
        let entry = LogParser::parse(input);
        assert_eq!(entry.level, Some(expected_level), "Failed for: {}", input);
    }
}

#[test]
fn test_log_parser_with_timestamp() {
    let entry = LogParser::parse("[2026-08-11 10:30:45] INFO: Starting application");
    assert_eq!(entry.level, Some(LogLevel::Info));
    assert!(entry.timestamp.is_some());
}

#[test]
fn test_log_parser_plain_text() {
    let entry = LogParser::parse("Just a plain message without level");
    assert!(entry.level.is_none());
    assert_eq!(entry.message, "Just a plain message without level");
}

#[test]
fn test_log_buffer_capacity() {
    let mut viewer = LogViewer::new(10);
    for i in 0..20 {
        viewer.add_line(&format!("INFO: Line {}", i));
    }
    // 缓冲区应该限制在10条
    assert!(viewer.buffer().len() <= 10);
}

#[test]
fn test_level_filter() {
    let mut viewer = LogViewer::new(100);
    viewer.add_line("INFO: info message");
    viewer.add_line("ERROR: error message");
    viewer.add_line("DEBUG: debug message");

    // 只显示 ERROR - 关闭所有其他级别
    viewer.toggle_level(LogLevel::Info);
    viewer.toggle_level(LogLevel::Debug);
    viewer.toggle_level(LogLevel::Trace);
    viewer.toggle_level(LogLevel::Warn);
    viewer.toggle_level(LogLevel::Fatal);

    let filtered: Vec<_> = viewer.filtered_entries().collect();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].level, Some(LogLevel::Error));
}

#[test]
fn test_auto_scroll() {
    let mut viewer = LogViewer::new(100);
    assert!(viewer.auto_scroll_enabled());

    // 向上滚动应该禁用自动滚动
    viewer.scroll_up(5);
    assert!(!viewer.auto_scroll_enabled());

    // 滚动到底部应该重新启用自动滚动
    viewer.scroll_to_bottom();
    assert!(viewer.auto_scroll_enabled());
}

#[test]
fn test_pause_functionality() {
    let mut viewer = LogViewer::new(100);
    viewer.add_line("INFO: Line 1");
    assert_eq!(viewer.buffer().len(), 1);

    // 暂停后不应添加新日志
    viewer.toggle_pause();
    assert!(viewer.is_paused());
    viewer.add_line("INFO: Line 2");
    assert_eq!(viewer.buffer().len(), 1);

    // 恢复后应该添加日志
    viewer.toggle_pause();
    assert!(!viewer.is_paused());
    viewer.add_line("INFO: Line 3");
    assert_eq!(viewer.buffer().len(), 2);
}

#[test]
fn test_search_filter() {
    let mut viewer = LogViewer::new(100);
    viewer.add_line("INFO: hello world");
    viewer.add_line("ERROR: something failed");
    viewer.add_line("INFO: hello again");
    viewer.add_line("DEBUG: goodbye world");

    // 搜索 "hello"
    viewer.set_search(Some("hello".to_string()));
    let filtered: Vec<_> = viewer.filtered_entries().collect();
    assert_eq!(filtered.len(), 2);

    // 清除搜索
    viewer.set_search(None);
    let filtered: Vec<_> = viewer.filtered_entries().collect();
    assert_eq!(filtered.len(), 4);
}

#[test]
fn test_level_colors() {
    assert_eq!(LogLevel::Error.color(), ratatui::style::Color::Red);
    assert_eq!(LogLevel::Warn.color(), ratatui::style::Color::Yellow);
    assert_eq!(LogLevel::Info.color(), ratatui::style::Color::Green);
    assert_eq!(LogLevel::Debug.color(), ratatui::style::Color::Gray);
}

#[test]
fn test_combined_filters() {
    let mut viewer = LogViewer::new(100);
    viewer.add_line("INFO: hello world");
    viewer.add_line("ERROR: hello failed");
    viewer.add_line("INFO: goodbye world");
    viewer.add_line("ERROR: goodbye failed");

    // 只显示ERROR级别
    viewer.toggle_level(LogLevel::Info);
    viewer.toggle_level(LogLevel::Debug);
    viewer.toggle_level(LogLevel::Trace);
    viewer.toggle_level(LogLevel::Warn);
    viewer.toggle_level(LogLevel::Fatal);

    // 并且搜索 "hello"
    viewer.set_search(Some("hello".to_string()));

    let filtered: Vec<_> = viewer.filtered_entries().collect();
    assert_eq!(filtered.len(), 1); // 只有 "ERROR: hello failed"
    assert_eq!(filtered[0].level, Some(LogLevel::Error));
    assert!(filtered[0].message.contains("hello"));
}

#[test]
fn test_buffer_statistics() {
    let mut viewer = LogViewer::new(100);
    viewer.add_line("INFO: info 1");
    viewer.add_line("INFO: info 2");
    viewer.add_line("ERROR: error 1");
    viewer.add_line("WARN: warning 1");
    viewer.add_line("DEBUG: debug 1");

    let stats = viewer.buffer().stats();
    assert_eq!(stats.total, 5);
    assert_eq!(stats.info, 2);
    assert_eq!(stats.error, 1);
    assert_eq!(stats.warn, 1);
    assert_eq!(stats.debug, 1);
}
