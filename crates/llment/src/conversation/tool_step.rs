use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use textwrap::wrap;

use super::node::ConvNode;

pub struct ToolStep {
    pub(crate) name: String,
    pub(crate) id: String,
    pub(crate) args: String,
    pub(crate) result: String,
    pub(crate) collapsed: bool,
    pub(crate) done: bool,
    pub(crate) failed: bool,
    cache_width: u16,
    cache_rev: u64,
    pub(crate) content_rev: u64,
    lines: Vec<Line<'static>>,
}

impl ToolStep {
    pub fn new(name: String, id: String, args: String, result: String, collapsed: bool) -> Self {
        Self {
            name,
            id,
            args,
            result,
            collapsed,
            done: false,
            failed: false,
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
        let mut lines = Vec::new();
        let arrow = if self.collapsed { "›" } else { "⌄" };
        let mut name_style = Style::default().add_modifier(Modifier::ITALIC | Modifier::UNDERLINED);
        if self.failed {
            name_style = name_style.fg(Color::Red);
        }
        lines.push(Line::from(vec![
            Span::raw("· "),
            Span::styled(self.name.clone(), name_style),
            Span::raw(format!(" {}", arrow)),
        ]));
        if !self.collapsed {
            let a_wrap = wrap(&self.args, width.saturating_sub(8) as usize);
            for (i, w) in a_wrap.into_iter().enumerate() {
                if i == 0 {
                    lines.push(Line::from(format!("│ args: {}", w)));
                } else {
                    lines.push(Line::from(format!("│       {}", w)));
                }
            }
            let r_wrap = wrap(&self.result, width.saturating_sub(10) as usize);
            for (i, w) in r_wrap.into_iter().enumerate() {
                if i == 0 {
                    lines.push(Line::from(format!("│ result: {}", w)));
                } else {
                    lines.push(Line::from(format!("│         {}", w)));
                }
            }
        }
        self.lines = lines;
    }
}

impl ConvNode for ToolStep {
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

    fn activate(&mut self) {
        self.collapsed = !self.collapsed;
        self.content_rev += 1;
    }
}
