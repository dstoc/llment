use crossterm::event::Event;
use ratatui::{Frame, layout::Rect};

pub trait Component {
    fn init(&mut self) {}
    fn handle_event(&mut self, _event: Event) {}
    fn update(&mut self) {}
    fn render(&mut self, f: &mut Frame, rect: Rect);
}
