use crate::component::Component;
use crossterm::event::{Event, MouseButton, MouseEventKind};
use futures_signals::signal::Mutable;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};
use textwrap::wrap;

/// Displays an error message in a dismissable box with an `x` button.
pub struct ErrorPopup {
    message: Option<String>,
    area: Rect,
    needs_redraw: Mutable<bool>,
}

impl ErrorPopup {
    pub fn new(needs_redraw: Mutable<bool>) -> Self {
        Self {
            message: None,
            area: Rect::default(),
            needs_redraw,
        }
    }

    pub fn set(&mut self, msg: String) {
        self.message = Some(msg);
        self.needs_redraw.set(true);
    }

    pub fn height(&self, width: u16) -> u16 {
        if let Some(msg) = &self.message {
            let inner = width.saturating_sub(2).max(1) as usize;
            wrap(msg, inner).len() as u16 + 2
        } else {
            0
        }
    }
}

impl Component for ErrorPopup {
    fn handle_event(&mut self, event: Event) {
        if self.message.is_none() {
            return;
        }
        if let Event::Mouse(me) = event {
            if me.kind == MouseEventKind::Down(MouseButton::Left) {
                let x_start = self.area.x + self.area.width.saturating_sub(2);
                let x_end = self.area.x + self.area.width - 1;
                if me.column >= x_start && me.column <= x_end && me.row == self.area.y {
                    self.message = None;
                    self.needs_redraw.set(true);
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        self.area = area;
        if let Some(msg) = &self.message {
            let inner = area.width.saturating_sub(2).max(1) as usize;
            let lines = wrap(msg, inner)
                .into_iter()
                .map(|l| ratatui::text::Line::from(l.into_owned()))
                .collect::<Vec<_>>();
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red));
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(lines)
                    .block(block)
                    .style(Style::default().fg(Color::Red)),
                area,
            );
            if area.width > 1 {
                frame.render_widget(
                    Paragraph::new("x").style(Style::default().fg(Color::Red)),
                    Rect::new(area.x + area.width - 2, area.y, 1, 1),
                );
            }
        }
    }
}
