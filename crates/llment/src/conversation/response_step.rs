use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Paragraph,
};

use super::node::ConvNode;
use crate::markdown::markdown_to_lines;

pub struct ResponseStep {
    pub(crate) text: String,
    cache_width: u16,
    cache_rev: u64,
    pub(crate) content_rev: u64,
    lines: Vec<Line<'static>>,
}

impl ResponseStep {
    pub fn new(text: String) -> Self {
        Self {
            text,
            cache_width: 0,
            cache_rev: 0,
            content_rev: 0,
            lines: Vec::new(),
        }
    }

    fn ensure_cache(&mut self, width: u16) {
        if self.cache_width == width && self.cache_rev == self.content_rev {
            return;
        }
        self.cache_width = width;
        self.cache_rev = self.content_rev;
        self.lines = markdown_to_lines(&self.text, width as usize);
        self.lines.push(Line::default());
    }
}

impl ConvNode for ResponseStep {
    fn height(&mut self, width: u16) -> u16 {
        self.ensure_cache(width);
        self.lines.len() as u16
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
        let style = if selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let start = start as usize;
        let end = (start + max_height as usize).min(self.lines.len());
        let lines: Vec<Line> = self.lines[start..end]
            .iter()
            .cloned()
            .map(|l| l.patch_style(style))
            .collect();
        let para = Paragraph::new(lines);
        frame.render_widget(para, area);
    }
}
