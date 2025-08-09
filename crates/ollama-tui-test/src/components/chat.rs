use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use textwrap::wrap;
use tuirealm::{
    AttrValue, Attribute, Component, Event, MockComponent, State,
    command::{Cmd, CmdResult},
};

use crate::markdown;

#[derive(Clone)]
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

#[derive(Clone)]
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

#[derive(Default, Clone)]
pub struct DrawState {
    pub history_rect: Rect,
    pub line_map: Vec<LineMapping>,
    pub top_line: usize,
}

pub struct Chat {
    pub items: Vec<HistoryItem>,
    pub scroll: i32,
    draw_state: DrawState,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            scroll: 0,
            draw_state: DrawState::default(),
        }
    }
}

impl Chat {
    pub fn draw_state(&self) -> &DrawState {
        &self.draw_state
    }
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
                                for w in wrapped {
                                    lines.push(format!("· {}", w));
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
                                collapsed,
                            } => {
                                let arrow = if *collapsed { "›" } else { "⌄" };
                                lines.push(format!("· _{name}_ {arrow}"));
                                mapping.push(LineMapping::Step {
                                    item: idx,
                                    step: s_idx,
                                });
                                markdown.push(true);
                                error.push(!success);
                                if !*collapsed {
                                    lines.push(format!("  args: {args}"));
                                    mapping.push(LineMapping::Step {
                                        item: idx,
                                        step: s_idx,
                                    });
                                    markdown.push(false);
                                    error.push(false);
                                    lines.push(format!("  result: {result}"));
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
                    lines.push(String::new());
                    mapping.push(LineMapping::Item(idx));
                    markdown.push(false);
                    error.push(false);
                }
            }
            HistoryItem::Separator => {
                lines.push(String::new());
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                error.push(false);
            }
        }
    }
    (lines, mapping, markdown, error)
}

pub fn apply_scroll(line_count: usize, height: usize, scroll_offset: &mut i32) -> (usize, usize) {
    let max_scroll = line_count.saturating_sub(height);
    *scroll_offset = (*scroll_offset).clamp(0, max_scroll as i32);
    (
        line_count.saturating_sub(height + *scroll_offset as usize),
        max_scroll,
    )
}

impl MockComponent for Chat {
    fn view(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)].as_ref())
            .split(area);
        let content = chunks[0];
        let width = content.width as usize;
        let (lines, mapping, markdown_flags, error_flags) = wrap_history_lines(&self.items, width);
        let mut rendered_lines = Vec::new();
        let mut rendered_map = Vec::new();
        for (((line, map), &md), &err) in lines
            .into_iter()
            .zip(mapping.into_iter())
            .zip(markdown_flags.iter())
            .zip(error_flags.iter())
        {
            if md {
                let mut converted = markdown::markdown_to_lines(&line, width);
                if err {
                    if let Some(first) = converted.first_mut() {
                        let styled = first.clone().patch_style(
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::ITALIC),
                        );
                        *first = styled;
                    }
                }
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
        let height = content.height as usize;
        let line_count = rendered_lines.len();
        let (top_line, max_scroll) = apply_scroll(line_count, height, &mut self.scroll);
        let paragraph = Paragraph::new(rendered_lines)
            .wrap(Wrap { trim: false })
            .scroll((top_line as u16, 0));
        f.render_widget(paragraph, content);
        if line_count > height {
            let mut scrollbar_state = ScrollbarState::new((max_scroll as usize) + 1)
                .position(top_line)
                .viewport_content_length(height);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
            f.render_stateful_widget(scrollbar, chunks[1], &mut scrollbar_state);
        }
        self.draw_state = DrawState {
            history_rect: content,
            line_map: rendered_map,
            top_line,
        };
    }

    fn query(&self, _: Attribute) -> Option<AttrValue> {
        None
    }

    fn attr(&mut self, _: Attribute, _: AttrValue) {}

    fn state(&self) -> State {
        State::None
    }

    fn perform(&mut self, _: Cmd) -> CmdResult {
        CmdResult::None
    }
}

impl Component<(), ()> for Chat {
    fn on(&mut self, _ev: Event<()>) -> Option<()> {
        None
    }
}
