use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use textwrap::wrap;
use unicode_width::UnicodeWidthChar;

use crate::markdown;
use tui_input::Input;

pub enum HistoryItem {
    User(String),
    Assistant(String),
    Thinking {
        steps: Vec<ThinkingStep>,
        collapsed: bool,
        start: Instant,
        duration: Duration,
        done: bool,
    },
    Separator,
    Error(String),
}

pub enum ThinkingStep {
    Thought(String),
    ToolCall {
        name: String,
        args: String,
        result: String,
        success: bool,
        collapsed: bool,
    },
}

#[derive(Clone, Copy)]
pub enum LineMapping {
    Item(usize),
    Step { item: usize, step: usize },
}

pub fn wrap_history_lines(
    items: &[HistoryItem],
    width: usize,
) -> (Vec<String>, Vec<LineMapping>, Vec<bool>, Vec<bool>) {
    let mut lines = Vec::new();
    let mut mapping = Vec::new();
    let mut markdown = Vec::new();
    let mut error = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        match item {
            HistoryItem::User(text) => {
                let inner_width = width.saturating_sub(7);
                let wrapped = wrap(text, inner_width.max(1));
                let box_width = wrapped.iter().map(|l| l.len()).max().unwrap_or(0);
                lines.push(format!("     ┌{}┐", "─".repeat(box_width)));
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                error.push(false);
                for w in wrapped {
                    let mut line = w.into_owned();
                    line.push_str(&" ".repeat(box_width.saturating_sub(line.len())));
                    lines.push(format!("     │{}│", line));
                    mapping.push(LineMapping::Item(idx));
                    markdown.push(false);
                    error.push(false);
                }
                lines.push(format!("     └{}┘", "─".repeat(box_width)));
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                error.push(false);
                lines.push(String::new());
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                error.push(false);
            }
            HistoryItem::Assistant(text) => {
                lines.push(text.clone());
                mapping.push(LineMapping::Item(idx));
                markdown.push(true);
                error.push(false);
            }
            HistoryItem::Error(text) => {
                lines.push(text.clone());
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                error.push(true);
            }
            HistoryItem::Thinking {
                steps,
                collapsed,
                duration,
                done,
                ..
            } => {
                let calls = steps
                    .iter()
                    .filter(|s| matches!(s, ThinkingStep::ToolCall { .. }))
                    .count();
                if *done {
                    let summary = format!(
                        "Thought for {} seconds, {calls} tool call{}",
                        duration.as_secs(),
                        if calls == 1 { "" } else { "s" },
                    );
                    let arrow = if *collapsed { "›" } else { "⌄" };
                    lines.push(format!("{summary} {arrow}"));
                } else {
                    lines.push("Thinking ⌄".to_string());
                }
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                error.push(false);
                if !*collapsed || !*done {
                    for (s_idx, step) in steps.iter().enumerate() {
                        match step {
                            ThinkingStep::Thought(t) => {
                                let wrapped = wrap(t, width.saturating_sub(2).max(1));
                                for (i, w) in wrapped.into_iter().enumerate() {
                                    if i == 0 {
                                        lines.push(format!("· {}", w));
                                    } else {
                                        lines.push(format!("  {}", w));
                                    }
                                    mapping.push(LineMapping::Step {
                                        item: idx,
                                        step: s_idx,
                                    });
                                    markdown.push(false);
                                    error.push(false);
                                }
                            }
                            ThinkingStep::ToolCall {
                                name,
                                args,
                                result,
                                success,
                                collapsed: tc_collapsed,
                            } => {
                                if *tc_collapsed {
                                    if *success {
                                        lines.push(format!("· _{name}_ ›"));
                                        mapping.push(LineMapping::Step {
                                            item: idx,
                                            step: s_idx,
                                        });
                                        markdown.push(true);
                                        error.push(false);
                                    } else {
                                        let line = format!("· {name} ›");
                                        lines.push(line);
                                        mapping.push(LineMapping::Step {
                                            item: idx,
                                            step: s_idx,
                                        });
                                        markdown.push(false);
                                        error.push(true);
                                    }
                                } else {
                                    if *success {
                                        lines.push(format!("· _{name}_ ⌄"));
                                        mapping.push(LineMapping::Step {
                                            item: idx,
                                            step: s_idx,
                                        });
                                        markdown.push(true);
                                        error.push(false);
                                    } else {
                                        let line = format!("· {name} ⌄");
                                        lines.push(line);
                                        mapping.push(LineMapping::Step {
                                            item: idx,
                                            step: s_idx,
                                        });
                                        markdown.push(false);
                                        error.push(true);
                                    }
                                    for w in wrap(
                                        &format!("args: {args}"),
                                        width.saturating_sub(2).max(1),
                                    ) {
                                        lines.push(format!("  {}", w));
                                        mapping.push(LineMapping::Step {
                                            item: idx,
                                            step: s_idx,
                                        });
                                        markdown.push(false);
                                        error.push(false);
                                    }
                                    let prefix = if *success { "result:" } else { "error:" };
                                    for w in wrap(
                                        &format!("{prefix} {result}"),
                                        width.saturating_sub(2).max(1),
                                    ) {
                                        lines.push(format!("  {}", w));
                                        mapping.push(LineMapping::Step {
                                            item: idx,
                                            step: s_idx,
                                        });
                                        markdown.push(false);
                                        error.push(false);
                                    }
                                }
                            }
                        }
                    }
                }
                lines.push(String::new());
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                error.push(false);
            }
            HistoryItem::Separator => {
                lines.push("─".repeat(width));
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                error.push(false);
            }
        }
    }
    (lines, mapping, markdown, error)
}

pub fn apply_scroll(
    line_count: usize,
    viewport_height: usize,
    scroll_offset: &mut i32,
) -> (usize, i32) {
    let max_scroll = line_count.saturating_sub(viewport_height) as i32;
    *scroll_offset = (*scroll_offset).clamp(0, max_scroll);
    let top_line = (max_scroll - *scroll_offset) as usize;
    (top_line, max_scroll)
}

#[derive(Default)]
pub struct DrawState {
    pub history_rect: Rect,
    pub line_map: Vec<LineMapping>,
    pub top_line: usize,
}

pub fn draw_ui(
    f: &mut Frame,
    items: &[HistoryItem],
    input: &Input,
    scroll_offset: &mut i32,
) -> DrawState {
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

    let history_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(content_width), Constraint::Length(1)].as_ref())
        .split(chunks[0]);

    let width = content_width as usize;
    let (lines, mapping, markdown_flags, error_flags) = wrap_history_lines(items, width);
    let mut rendered_lines = Vec::new();
    let mut rendered_map = Vec::new();
    for (((line, map), &md), &err) in lines
        .into_iter()
        .zip(mapping.into_iter())
        .zip(markdown_flags.iter())
        .zip(error_flags.iter())
    {
        if md {
            let converted = markdown::markdown_to_lines(&line, width);
            rendered_map.extend(std::iter::repeat(map).take(converted.len()));
            rendered_lines.extend(converted);
        } else if err {
            rendered_map.push(map);
            rendered_lines.push(Line::styled(
                line,
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::ITALIC),
            ));
        } else {
            rendered_map.push(map);
            rendered_lines.push(Line::raw(line));
        }
    }
    let history_height = history_chunks[0].height as usize;
    let line_count = rendered_lines.len();
    let (top_line, max_scroll) = apply_scroll(line_count, history_height, scroll_offset);

    let paragraph = Paragraph::new(rendered_lines)
        .wrap(Wrap { trim: false })
        .scroll((top_line as u16, 0));
    f.render_widget(paragraph, history_chunks[0]);

    if line_count > history_height {
        let mut scrollbar_state = ScrollbarState::new((max_scroll as usize) + 1)
            .position(top_line)
            .viewport_content_length(history_height);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        f.render_stateful_widget(scrollbar, history_chunks[1], &mut scrollbar_state);
    }

    let mut display_lines = Vec::new();
    if let Some((first, rest)) = input_lines.split_first() {
        display_lines.push(Line::raw(format!("> {}", first)));
        for line in rest {
            display_lines.push(Line::raw(format!("  {}", line)));
        }
    }
    let input_widget = Paragraph::new(display_lines);
    f.render_widget(input_widget, chunks[1]);

    let cursor_pos = input.cursor();
    let mut x = 0u16;
    let mut y = 0u16;
    for c in input.value().chars().take(cursor_pos) {
        if c == '\n' {
            y += 1;
            x = 0;
        } else {
            x += c.width().unwrap_or(0) as u16;
        }
    }
    f.set_cursor_position((chunks[1].x + 2 + x, chunks[1].y + y));

    DrawState {
        history_rect: history_chunks[0],
        line_map: rendered_map,
        top_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        style::{Color, Modifier},
    };
    use tui_input::Input;

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
    fn draw_ui_scrolls_history_snapshots() {
        let backend = TestBackend::new(20, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = (0..6)
            .map(|i| HistoryItem::Assistant(format!("line{i}")))
            .collect::<Vec<_>>();
        let mut scroll = i32::MAX;
        let input = Input::default();

        terminal
            .draw(|f| {
                draw_ui(f, &items, &input, &mut scroll);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let top_rendered = buffer_to_string(&buffer);
        assert_snapshot!(
            top_rendered,
            @r###"
line0              ▲
line1              █
line2              ║
line3              ▼
>                   
                    
                    
"###
        );

        scroll = 0;
        terminal
            .draw(|f| {
                draw_ui(f, &items, &input, &mut scroll);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let bottom_rendered = buffer_to_string(&buffer);
        assert_snapshot!(
            bottom_rendered,
            @r###"
line2              ▲
line3              ║
line4              █
line5              ▼
>


"###
        );
    }

    #[test]
    fn draw_ui_renders_user_message() {
        let backend = TestBackend::new(20, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![HistoryItem::User("Hello".into())];
        let mut scroll = 0;
        let input = Input::default();
        terminal
            .draw(|f| {
                draw_ui(f, &items, &input, &mut scroll);
            })
            .unwrap();

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
            @r###"
     ┌─────┐
     │Hello│
     └─────┘

>
"###
        );
    }

    #[test]
    fn draw_ui_renders_assistant_message() {
        let backend = TestBackend::new(20, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![HistoryItem::Assistant("Hello".into())];
        let mut scroll = 0;
        let input = Input::default();
        terminal
            .draw(|f| {
                draw_ui(f, &items, &input, &mut scroll);
            })
            .unwrap();

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
            @r###"
Hello



>
"###
        );
    }

    #[test]
    fn input_box_expands_for_multiline() {
        let backend = TestBackend::new(20, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![];
        let mut scroll = 0;
        let input = Input::default().with_value("hello\nworld".into());
        terminal
            .draw(|f| {
                draw_ui(f, &items, &input, &mut scroll);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let rendered = buffer_to_string(&buffer);
        let trimmed = rendered
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        assert_snapshot!(trimmed, @r###"
> hello
  world
"###);
    }

    #[test]
    fn draw_ui_renders_thinking_block_with_tool_call() {
        let backend = TestBackend::new(20, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![HistoryItem::Thinking {
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
        }];
        let mut scroll = 0;
        let input = Input::default();
        terminal
            .draw(|f| {
                draw_ui(f, &items, &input, &mut scroll);
            })
            .unwrap();

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
            @r###"
Thinking ⌄
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
        let items = vec![HistoryItem::Assistant("Hi".into())];
        let mut scroll = 0;
        let input = Input::default();
        terminal
            .draw(|f| {
                draw_ui(f, &items, &input, &mut scroll);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rendered = buffer_to_string(&buffer);
        let first_line = rendered.lines().next().unwrap();
        assert_eq!(first_line.find("Hi"), Some(9));
    }

    #[test]
    fn failed_tool_call_heading_is_red_italic() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![HistoryItem::Thinking {
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
        }];
        let mut scroll = 0;
        let input = Input::default();
        terminal
            .draw(|f| {
                draw_ui(f, &items, &input, &mut scroll);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let cell = buffer.cell((0, 0)).unwrap();
        assert_eq!(cell.style().fg, Some(Color::Red));
        assert!(cell.style().add_modifier.contains(Modifier::ITALIC));
    }
}
