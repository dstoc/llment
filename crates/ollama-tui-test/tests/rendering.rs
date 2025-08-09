use std::time::{Duration, Instant};

use insta::assert_snapshot;
use ollama_tui_test::components::{
    chat::{Chat, HistoryItem, ThinkingStep},
    input::InputComponent,
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier},
};
use tui_input::Input;
use tuirealm::MockComponent;

fn buffer_to_string(buffer: &Buffer) -> String {
    let area = buffer.area;
    let mut lines = Vec::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buffer.cell((x, y)).unwrap().symbol());
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn draw(chat: &mut Chat, input: &mut InputComponent, terminal: &mut Terminal<TestBackend>) {
    terminal
        .draw(|f| {
            let area = f.area();
            let content_width = area.width.saturating_sub(1).min(100);
            let total_width = content_width + 1;
            let x_offset = (area.width.saturating_sub(total_width)) / 2;
            let centered = Rect::new(area.x + x_offset, area.y, total_width, area.height);
            let input_lines: Vec<&str> = input.value().split('\n').collect();
            let input_height = input_lines.len() as u16 + 2;
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(input_height)].as_ref())
                .split(centered);
            chat.view(f, chunks[0]);
            input.view(f, chunks[1]);
        })
        .unwrap();
}

#[test]
fn chat_scrolls_history_snapshots() {
    let backend = TestBackend::new(20, 7);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut chat = Chat::default();
    chat.items = (0..6)
        .map(|i| HistoryItem::Assistant(format!("line{i}")))
        .collect();
    chat.scroll = i32::MAX;
    let mut input = InputComponent::default();
    draw(&mut chat, &mut input, &mut terminal);
    let buffer = terminal.backend().buffer().clone();
    let top_rendered = buffer_to_string(&buffer);
    assert_snapshot!(
        top_rendered,
        @r###"line0              ▲
line1              █
line2              ║
line3              ▼
>

"###
    );

    chat.scroll = 0;
    draw(&mut chat, &mut input, &mut terminal);
    let buffer = terminal.backend().buffer().clone();
    let bottom_rendered = buffer_to_string(&buffer);
    assert_snapshot!(
        bottom_rendered,
        @r###"line2              ▲
line3              ║
line4              █
line5              ▼
>

"###
    );
}

#[test]
fn chat_renders_user_message() {
    let backend = TestBackend::new(20, 7);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut chat = Chat::default();
    chat.items.push(HistoryItem::User("Hello".into()));
    let mut input = InputComponent::default();
    draw(&mut chat, &mut input, &mut terminal);
    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer_to_string(&buffer);
    let trimmed = rendered
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string();
    assert_snapshot!(
        trimmed,
        @r###"     ┌─────┐
     │Hello│
     └─────┘

>
"###
    );
}

#[test]
fn chat_renders_assistant_message() {
    let backend = TestBackend::new(20, 7);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut chat = Chat::default();
    chat.items.push(HistoryItem::Assistant("Hello".into()));
    let mut input = InputComponent::default();
    draw(&mut chat, &mut input, &mut terminal);
    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer_to_string(&buffer);
    let trimmed = rendered
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string();
    assert_snapshot!(
        trimmed,
        @r###"Hello



>
"###
    );
}

#[test]
fn input_box_expands_for_multiline() {
    let backend = TestBackend::new(20, 7);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut chat = Chat::default();
    let mut input = InputComponent::default();
    input.input = Input::default().with_value("hello\nworld".into());
    draw(&mut chat, &mut input, &mut terminal);
    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer_to_string(&buffer);
    let trimmed = rendered
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    assert_snapshot!(trimmed, @r###"> hello
  world"###);
}

#[test]
fn chat_renders_thinking_block_with_tool_call() {
    let backend = TestBackend::new(20, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut chat = Chat::default();
    chat.items.push(HistoryItem::Thinking {
        steps: vec![ThinkingStep::ToolCall {
            name: "tool".into(),
            args: "{}".into(),
            result: "ok".into(),
            success: true,
            collapsed: false,
        }],
        collapsed: false,
        start: Instant::now(),
        duration: Duration::from_secs(0),
        done: false,
    });
    let mut input = InputComponent::default();
    draw(&mut chat, &mut input, &mut terminal);
    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer_to_string(&buffer);
    let trimmed = rendered
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string();
    assert_snapshot!(
        trimmed,
        @r###"Thinking ⌄
· _tool_ ⌄
  args: {}
  result: ok

>
"###
    );
}

#[test]
fn centers_chat_content() {
    let backend = TestBackend::new(120, 7);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut chat = Chat::default();
    chat.items.push(HistoryItem::Assistant("Hi".into()));
    let mut input = InputComponent::default();
    draw(&mut chat, &mut input, &mut terminal);
    let buffer = terminal.backend().buffer().clone();
    let rendered = buffer_to_string(&buffer);
    let first_line = rendered.lines().next().unwrap();
    assert_eq!(first_line.find("Hi"), Some(9));
}

#[test]
fn failed_tool_call_heading_is_red_italic() {
    let backend = TestBackend::new(20, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut chat = Chat::default();
    chat.items.push(HistoryItem::Thinking {
        steps: vec![ThinkingStep::ToolCall {
            name: "tool".into(),
            args: "{}".into(),
            result: "bad".into(),
            success: false,
            collapsed: true,
        }],
        collapsed: false,
        start: Instant::now(),
        duration: Duration::from_secs(0),
        done: false,
    });
    let mut input = InputComponent::default();
    draw(&mut chat, &mut input, &mut terminal);
    let buffer = terminal.backend().buffer().clone();
    let cell = buffer.cell((0, 0)).unwrap();
    assert_eq!(cell.style().fg, Some(Color::Red));
    assert!(cell.style().add_modifier.contains(Modifier::ITALIC));
}
