use ratatui::{Frame, layout::Rect, text::Line, widgets::Paragraph};
use tui_input::{Input, InputRequest, backend::crossterm::EventHandler as _};
use tuirealm::{
    AttrValue, Attribute, Component, Event, MockComponent, State,
    command::{Cmd, CmdResult},
};
use unicode_width::UnicodeWidthChar;

pub struct InputComponent {
    pub input: Input,
}

impl Default for InputComponent {
    fn default() -> Self {
        Self {
            input: Input::default(),
        }
    }
}

impl InputComponent {
    pub fn reset(&mut self) {
        self.input.reset();
    }

    pub fn value(&self) -> &str {
        self.input.value()
    }

    pub fn handle(&mut self, req: InputRequest) {
        self.input.handle(req);
    }

    pub fn handle_event(&mut self, ev: &crossterm::event::Event) {
        self.input.handle_event(ev);
    }
}

impl MockComponent for InputComponent {
    fn view(&mut self, f: &mut Frame, area: Rect) {
        let lines: Vec<&str> = self.input.value().split('\n').collect();
        let mut display_lines = Vec::new();
        if let Some((first, rest)) = lines.split_first() {
            display_lines.push(Line::raw(format!("> {}", first)));
            for line in rest {
                display_lines.push(Line::raw(format!("  {}", line)));
            }
        }
        let input_widget = Paragraph::new(display_lines);
        f.render_widget(input_widget, area);
        let cursor_pos = self.input.cursor();
        let mut x = 0u16;
        let mut y = 0u16;
        for c in self.input.value().chars().take(cursor_pos) {
            if c == '\n' {
                y += 1;
                x = 0;
            } else {
                x += c.width().unwrap_or(0) as u16;
            }
        }
        f.set_cursor_position((area.x + 2 + x, area.y + y));
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

impl Component<(), ()> for InputComponent {
    fn on(&mut self, _ev: Event<()>) -> Option<()> {
        None
    }
}
