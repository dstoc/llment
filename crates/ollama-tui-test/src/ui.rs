use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use textwrap::wrap;

use crate::markdown;

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
) -> (Vec<String>, Vec<LineMapping>, Vec<bool>) {
    let mut lines = Vec::new();
    let mut mapping = Vec::new();
    let mut markdown = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        match item {
            HistoryItem::User(text) => {
                let inner_width = width.saturating_sub(7);
                let wrapped = wrap(text, inner_width.max(1));
                let box_width = wrapped.iter().map(|l| l.len()).max().unwrap_or(0);
                lines.push(format!("     ┌{}┐", "─".repeat(box_width)));
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                for w in wrapped {
                    let mut line = w.into_owned();
                    line.push_str(&" ".repeat(box_width.saturating_sub(line.len())));
                    lines.push(format!("     │{}│", line));
                    mapping.push(LineMapping::Item(idx));
                    markdown.push(false);
                }
                lines.push(format!("     └{}┘", "─".repeat(box_width)));
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                lines.push(String::new());
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
            }
            HistoryItem::Assistant(text) => {
                lines.push(text.clone());
                mapping.push(LineMapping::Item(idx));
                markdown.push(true);
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
                                    lines.push(format!("· _{name}_ ›"));
                                    mapping.push(LineMapping::Step {
                                        item: idx,
                                        step: s_idx,
                                    });
                                    markdown.push(true);
                                } else {
                                    lines.push(format!("· _{name}_ ⌄"));
                                    mapping.push(LineMapping::Step {
                                        item: idx,
                                        step: s_idx,
                                    });
                                    markdown.push(true);
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
                                    }
                                }
                            }
                        }
                    }
                }
                lines.push(String::new());
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
            }
            HistoryItem::Separator => {
                lines.push("─".repeat(width));
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
            }
        }
    }
    (lines, mapping, markdown)
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
    input: &str,
    scroll_offset: &mut i32,
) -> DrawState {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
        .split(area);

    let history_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)].as_ref())
        .split(chunks[0]);

    let width = history_chunks[0].width as usize;
    let (lines, mapping, markdown_flags) = wrap_history_lines(items, width);
    let mut rendered_lines = Vec::new();
    let mut rendered_map = Vec::new();
    for ((line, map), &md) in lines
        .into_iter()
        .zip(mapping.into_iter())
        .zip(markdown_flags.iter())
    {
        if md {
            let converted = markdown::markdown_to_lines(&line, width);
            rendered_map.extend(std::iter::repeat(map).take(converted.len()));
            rendered_lines.extend(converted);
        } else {
            rendered_map.push(map);
            rendered_lines.push(Line::raw(line));
        }
    }
    let history_height = history_chunks[0].height as usize;
    let line_count = rendered_lines.len();
    let max_scroll = line_count.saturating_sub(history_height) as i32;
    *scroll_offset = (*scroll_offset).clamp(0, max_scroll);
    let top_line = (max_scroll - *scroll_offset) as usize;

    let paragraph = Paragraph::new(rendered_lines)
        .wrap(Wrap { trim: false })
        .scroll((top_line as u16, 0));
    f.render_widget(paragraph, history_chunks[0]);

    let mut scrollbar_state = ScrollbarState::new(line_count)
        .position(top_line)
        .viewport_content_length(history_height);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    f.render_stateful_widget(scrollbar, history_chunks[1], &mut scrollbar_state);

    let input_widget = Paragraph::new(format!("> {}", input));
    f.render_widget(input_widget, chunks[1]);

    DrawState {
        history_rect: history_chunks[0],
        line_map: rendered_map,
        top_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    #[test]
    fn draw_ui_renders_user_message() {
        let backend = TestBackend::new(20, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![HistoryItem::User("Hello".into())];
        let mut scroll = 0;
        terminal
            .draw(|f| {
                draw_ui(f, &items, "", &mut scroll);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let expected = Buffer::with_lines(vec![
            "     ┌─────┐       ▲",
            "     │Hello│       █",
            "     └─────┘       ║",
            "                   ▼",
            ">                   ",
            "                    ",
            "                    ",
        ]);
        assert_eq!(buffer, expected);
    }

    #[test]
    fn draw_ui_renders_assistant_message() {
        let backend = TestBackend::new(20, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![HistoryItem::Assistant("Hello".into())];
        let mut scroll = 0;
        terminal
            .draw(|f| {
                draw_ui(f, &items, "", &mut scroll);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let expected = Buffer::with_lines(vec![
            "Hello              ▲",
            "                   █",
            "                   █",
            "                   ▼",
            ">                   ",
            "                    ",
            "                    ",
        ]);
        assert_eq!(buffer, expected);
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
        terminal
            .draw(|f| {
                draw_ui(f, &items, "", &mut scroll);
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let expected = Buffer::with_lines(vec![
            "Thinking ⌄         ▲",
            "· _tool_ ⌄         █",
            "  args: {}         █",
            "  result: ok       ║",
            "                   ▼",
            ">                   ",
            "                    ",
            "                    ",
        ]);
        assert_eq!(buffer, expected);
    }
}
