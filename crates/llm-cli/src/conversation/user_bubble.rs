use textwrap::wrap;
use tuirealm::ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::node::ConvNode;

pub struct UserBubble {
    pub(crate) text: String,
    cache_width: u16,
    cache_rev: u64,
    pub(crate) content_rev: u64,
    lines: Vec<String>,
    box_width: u16,
}

impl UserBubble {
    pub fn new(text: String) -> Self {
        Self {
            text,
            cache_width: 0,
            cache_rev: 0,
            content_rev: 0,
            lines: Vec::new(),
            box_width: 0,
        }
    }

    fn ensure_cache(&mut self, width: u16) {
        if self.cache_width == width && self.cache_rev == self.content_rev {
            return;
        }
        self.cache_width = width;
        self.cache_rev = self.content_rev;
        let inner = width.saturating_sub(2) as usize;
        let wrapped = wrap(&self.text, inner.max(1));
        let content_width = wrapped
            .iter()
            .map(|l| UnicodeWidthStr::width(l.as_ref()) as u16)
            .max()
            .unwrap_or(0);
        self.box_width = (content_width + 2).min(width);
        self.lines = wrapped.into_iter().map(|w| w.into_owned()).collect();
    }
}

impl ConvNode for UserBubble {
    fn height(&mut self, width: u16) -> u16 {
        self.ensure_cache(width);
        self.lines.len() as u16 + 3
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
        frame.render_widget(Clear, area);
        let bubble_height = self.lines.len() as u16 + 2;
        if start >= bubble_height {
            return;
        }
        let style = if selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let width = self.box_width;
        let x = area.x + area.width.saturating_sub(width);
        let visible = max_height.min(bubble_height - start);
        let mut borders = Borders::ALL;
        if start > 0 {
            borders.remove(Borders::TOP);
        }
        if start + visible < bubble_height {
            borders.remove(Borders::BOTTOM);
        }
        let scroll = start.saturating_sub(1);
        let lines: Vec<Line> = self
            .lines
            .iter()
            .map(|l| Line::from(Span::raw(l.clone())))
            .collect();
        let block = Block::default()
            .borders(borders)
            .border_type(BorderType::Rounded)
            .border_style(style);
        let para = Paragraph::new(lines)
            .style(style)
            .block(block)
            .scroll((scroll, 0));
        frame.render_widget(
            para,
            Rect {
                x,
                y: area.y,
                width,
                height: visible,
            },
        );
    }
}
