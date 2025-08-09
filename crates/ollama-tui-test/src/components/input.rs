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
    fn on(&mut self, ev: Event<()>) -> Option<()> {
        use crossterm::event::{
            Event as CEvent, KeyCode as CKeyCode, KeyEvent as CKeyEvent,
            KeyModifiers as CKeyModifiers,
        };
        use tuirealm::event::{Key, KeyModifiers};
        match ev {
            Event::Keyboard(key) => {
                let ct_key = CKeyEvent::new(
                    match key.code {
                        Key::Char(c) => CKeyCode::Char(c),
                        Key::Enter => CKeyCode::Enter,
                        Key::Backspace => CKeyCode::Backspace,
                        Key::Left => CKeyCode::Left,
                        Key::Right => CKeyCode::Right,
                        Key::Up => CKeyCode::Up,
                        Key::Down => CKeyCode::Down,
                        _ => CKeyCode::Null,
                    },
                    CKeyModifiers::from_bits_truncate(key.modifiers.bits()),
                );
                match (key.code, key.modifiers) {
                    (Key::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => {
                        self.reset();
                    }
                    (Key::Char('j'), m) if m.contains(KeyModifiers::CONTROL) => {
                        self.input.handle(InputRequest::InsertChar('\n'));
                    }
                    _ => {
                        self.input.handle_event(&CEvent::Key(ct_key));
                    }
                }
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        layout::{Constraint, Direction, Layout},
    };
    use tuirealm::MockComponent;

    fn buffer_to_string(buffer: &Buffer) -> String {
        let area = buffer.area;
        let mut lines = Vec::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    #[test]
    fn input_box_expands_for_multiline() {
        let backend = TestBackend::new(20, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut input = InputComponent::default();
        input.input = Input::default().with_value("hello\nworld".into());
        terminal
            .draw(|f| {
                let area = f.area();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(2)].as_ref())
                    .split(area);
                input.view(f, chunks[1]);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rendered = buffer_to_string(&buffer);
        let trimmed = rendered
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        assert_snapshot!(trimmed, @r###"> hello
  world"###);
    }
}
