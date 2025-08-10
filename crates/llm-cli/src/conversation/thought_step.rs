use textwrap::wrap;
use tuirealm::ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::node::ConvNode;

pub struct ThoughtStep {
    pub(crate) text: String,
    cache_width: u16,
    cache_rev: u64,
    pub(crate) content_rev: u64,
    lines: Vec<String>,
}

impl ThoughtStep {
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
        let inner = width.saturating_sub(2) as usize;
        let wrapped = wrap(&self.text, inner.max(1));
        let mut lines = Vec::new();
        for (i, w) in wrapped.into_iter().enumerate() {
            if i == 0 {
                lines.push(format!("· {}", w));
            } else {
                lines.push(format!("  {}", w));
            }
        }
        self.lines = lines;
    }
}

impl ConvNode for ThoughtStep {
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
            .map(|l| Line::from(Span::styled(l.clone(), style)))
            .collect();
        let para = Paragraph::new(lines);
        frame.render_widget(para, area);
    }
}
