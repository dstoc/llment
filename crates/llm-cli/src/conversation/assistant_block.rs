use crate::markdown::markdown_to_lines;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::time::Instant;
use tui_realm_stdlib::states::SpinnerStates;

use super::{Node, node::ConvNode};

pub struct AssistantBlock {
    pub(crate) working_collapsed: bool,
    pub(crate) steps: Vec<Node>,
    pub(crate) response: String,
    cache_width: u16,
    cache_rev: u64,
    pub(crate) content_rev: u64,
    response_lines: Vec<Line<'static>>,
    pub(crate) selected: usize,
    started: Option<Instant>,
    last_update: Option<Instant>,
    spinner: SpinnerStates,
}

impl AssistantBlock {
    pub fn new(working_collapsed: bool, steps: Vec<Node>, response: String) -> Self {
        let mut spinner = SpinnerStates::default();
        spinner.reset("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");
        Self {
            working_collapsed,
            steps,
            response,
            cache_width: 0,
            cache_rev: 0,
            content_rev: 0,
            response_lines: Vec::new(),
            selected: 0,
            started: None,
            last_update: None,
            spinner,
        }
    }

    pub(crate) fn record_activity(&mut self) {
        let now = Instant::now();
        if self.started.is_none() {
            self.started = Some(now);
        }
        self.last_update = Some(now);
    }

    fn summary(&mut self) -> String {
        if self.response.is_empty() {
            let mut parts = vec!["Thinking".to_string()];
            let used = self
                .steps
                .iter()
                .filter(|s| matches!(s, Node::Tool(t) if t.done))
                .count();
            if used > 0 {
                parts.push(format!(
                    "used {used} tool{}",
                    if used == 1 { "" } else { "s" }
                ));
            }
            if let Some(t) = self.steps.last().and_then(|s| match s {
                Node::Tool(t) if !t.done => Some(t),
                _ => None,
            }) {
                parts.push(format!("using {}", t.name));
            }
            let mut summary = parts.join(", ");
            summary.push(' ');
            summary.push(self.spinner.step());
            summary
        } else {
            let mut parts = Vec::new();
            if let (Some(start), Some(end)) = (self.started, self.last_update) {
                let secs = end.duration_since(start).as_secs();
                parts.push(format!("Thought for {secs}s"));
            }
            let used = self
                .steps
                .iter()
                .filter(|s| matches!(s, Node::Tool(t) if t.done))
                .count();
            if used > 0 {
                parts.push(format!(
                    "used {used} tool{}",
                    if used == 1 { "" } else { "s" }
                ));
            }
            parts.join(", ")
        }
    }

    fn ensure_cache(&mut self, width: u16) {
        if self.cache_width == width && self.cache_rev == self.content_rev {
            return;
        }
        self.cache_width = width;
        self.cache_rev = self.content_rev;
        self.response_lines = markdown_to_lines(&self.response, width as usize);
        self.response_lines.push(Line::default());
    }

    fn total_items(&self) -> usize {
        let steps = if self.working_collapsed {
            0
        } else {
            self.steps.len()
        };
        1 + steps + 1
    }
}

impl ConvNode for AssistantBlock {
    fn height(&mut self, width: u16) -> u16 {
        self.ensure_cache(width);
        let mut h = 1;
        if !self.working_collapsed {
            for step in &mut self.steps {
                h += step.height(width);
            }
        }
        h += self.response_lines.len() as u16;
        h
    }

    fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        selected: bool,
        start: u16,
        max_height: u16,
    ) {
        self.ensure_cache(area.width);
        let arrow = if self.working_collapsed { "›" } else { "⌄" };
        let sel_style = |is_sel| {
            if selected && is_sel {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            }
        };

        let mut y = area.y;
        let mut remaining = max_height;
        let mut line_idx = 0u16;

        if start == 0 && remaining > 0 {
            let header = Paragraph::new(Line::from(Span::styled(
                format!("{} {arrow}", self.summary()),
                sel_style(self.selected == 0),
            )));
            frame.render_widget(header, Rect { height: 1, ..area });
            y += 1;
            remaining -= 1;
        }
        line_idx += 1;
        if remaining == 0 {
            return;
        }

        if !self.working_collapsed {
            for (i, step) in self.steps.iter_mut().enumerate() {
                let h = step.height(area.width);
                if line_idx + h <= start {
                    line_idx += h;
                    continue;
                }
                let offset = if start > line_idx {
                    start - line_idx
                } else {
                    0
                };
                let avail = remaining.min(h - offset);
                let rect = Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: avail,
                };
                step.render(
                    frame,
                    rect,
                    selected && self.selected == i + 1,
                    offset,
                    avail,
                );
                y += avail;
                remaining -= avail;
                line_idx += h;
                if remaining == 0 {
                    return;
                }
            }
        } else {
            line_idx += self
                .steps
                .iter_mut()
                .map(|s| s.height(area.width))
                .sum::<u16>();
        }

        if remaining > 0 {
            let resp_total = self.response_lines.len() as u16;
            if line_idx + resp_total <= start {
                return;
            }
            let offset = if start > line_idx {
                start - line_idx
            } else {
                0
            };
            let visible = remaining.min(resp_total - offset);
            let style = sel_style(self.selected == self.total_items() - 1);
            let start_idx = offset as usize;
            let end_idx = (start_idx + visible as usize).min(self.response_lines.len());
            let lines: Vec<Line> = self.response_lines[start_idx..end_idx]
                .iter()
                .cloned()
                .map(|l| l.style(style))
                .collect();
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: visible,
            };
            let para = Paragraph::new(lines);
            frame.render_widget(para, rect);
        }
    }

    fn activate(&mut self) {
        if self.selected == 0 {
            self.working_collapsed = !self.working_collapsed;
        } else {
            let idx = self.selected - 1;
            if idx < self.steps.len() {
                self.steps[idx].activate();
            }
        }
        self.content_rev += 1;
    }

    fn on_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Down => {
                let max = self.total_items() - 1;
                if self.selected < max {
                    self.selected += 1;
                    true
                } else {
                    false
                }
            }
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn click(&mut self, line: u16) {
        if line == 0 {
            self.selected = 0;
        } else {
            let mut pos = 1;
            if !self.working_collapsed {
                for (i, step) in self.steps.iter_mut().enumerate() {
                    let h = step.height(self.cache_width);
                    if line < pos + h {
                        self.selected = i + 1;
                        self.activate();
                        return;
                    }
                    pos += h;
                }
            }
            self.selected = self.total_items() - 1;
        }
        self.activate();
    }
}
