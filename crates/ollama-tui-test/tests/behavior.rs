use ollama_tui_test::components::chat::{
    HistoryItem, LineMapping, ThinkingStep, apply_scroll, wrap_history_lines,
};

#[test]
fn apply_scroll_clamps_bounds() {
    let mut scroll = -10;
    let (top, max) = apply_scroll(50, 10, &mut scroll);
    assert_eq!(scroll, 0);
    assert_eq!(top, 40);
    assert_eq!(max, 40);

    scroll = 100;
    let (top, max) = apply_scroll(50, 10, &mut scroll);
    assert_eq!(scroll, 40);
    assert_eq!(top, 0);
    assert_eq!(max, 40);
}

#[test]
fn wrap_history_lines_maps_tool_calls() {
    let items = vec![HistoryItem::Thinking {
        steps: vec![ThinkingStep::ToolCall {
            name: "tool".into(),
            args: "{}".into(),
            result: "ok".into(),
            success: true,
            collapsed: false,
        }],
        collapsed: false,
        start: std::time::Instant::now(),
        duration: std::time::Duration::from_secs(0),
        done: false,
    }];
    let (lines, mapping, _, _) = wrap_history_lines(&items, 20);
    assert_eq!(lines[0], "Thinking ⌄");
    assert!(matches!(mapping[1], LineMapping::Step { .. }));
}
