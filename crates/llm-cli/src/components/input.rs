use textwrap::wrap;
use tui_textarea::{Input as TaInput, Key as TaKey, TextArea};
use tuirealm::command::{Cmd, CmdResult};
use tuirealm::event::{Key, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tuirealm::props::{AttrValue, Attribute};
use tuirealm::ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Text,
    widgets::{Block, BorderType, Borders, Paragraph, Wrap as RtWrap},
};
use tuirealm::{Component, Event, Frame, MockComponent, NoUserEvent, State, StateValue};
use unicode_width::UnicodeWidthStr;

use crate::Msg;

/// Multiline prompt input backed by [`tui_textarea`].
pub struct Prompt {
    textarea: TextArea<'static>,
    area: Rect,
}

impl Prompt {
    fn new_textarea() -> TextArea<'static> {
        let mut ta = TextArea::default();
        ta.set_block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::LightBlue))
                .title("Input"),
        );
        ta.set_style(Style::default().fg(Color::LightBlue));
        ta
    }

    fn cursor_position(&self, inner: Rect) -> (u16, u16, u16) {
        let (row, col) = self.textarea.cursor();
        let width = inner.width as usize;
        let mut cursor_x = 0usize;
        let mut cursor_y = 0usize;

        for (idx, line) in self.textarea.lines().iter().enumerate() {
            let wrapped = wrap(line, width);
            if idx < row {
                cursor_y += wrapped.len();
                continue;
            }
            let mut remaining = col;
            for (widx, part) in wrapped.iter().enumerate() {
                let part_str = part.to_string();
                let chars = part_str.chars().count();
                if remaining <= chars {
                    let prefix: String = part_str.chars().take(remaining).collect();
                    cursor_x = UnicodeWidthStr::width(prefix.as_str());
                    cursor_y += widx;
                    return (
                        inner.x + cursor_x as u16,
                        inner.y + cursor_y as u16,
                        cursor_y as u16,
                    );
                }
                remaining -= chars;
                cursor_y += 1;
            }
            break;
        }
        (
            inner.x + cursor_x as u16,
            inner.y + cursor_y as u16,
            cursor_y as u16,
        )
    }

    fn set_block(&mut self) {
        // Reapply block/style after clearing
        self.textarea = Self::new_textarea();
    }
}

impl Default for Prompt {
    fn default() -> Self {
        Self {
            textarea: Self::new_textarea(),
            area: Rect::default(),
        }
    }
}

fn to_input(ev: KeyEvent) -> TaInput {
    let key = match ev.code {
        Key::Backspace => TaKey::Backspace,
        Key::Enter => TaKey::Enter,
        Key::Left => TaKey::Left,
        Key::Right => TaKey::Right,
        Key::Up => TaKey::Up,
        Key::Down => TaKey::Down,
        Key::Home => TaKey::Home,
        Key::End => TaKey::End,
        Key::Tab => TaKey::Tab,
        Key::Delete => TaKey::Delete,
        Key::Char(c) => TaKey::Char(c),
        _ => TaKey::Null,
    };
    TaInput {
        key,
        ctrl: ev.modifiers.contains(KeyModifiers::CONTROL),
        alt: ev.modifiers.contains(KeyModifiers::ALT),
        shift: ev.modifiers.contains(KeyModifiers::SHIFT),
    }
}

impl Component<Msg, NoUserEvent> for Prompt {
    fn on(&mut self, ev: Event<NoUserEvent>) -> Option<Msg> {
        match ev {
            Event::Keyboard(key) => match (key.code, key.modifiers) {
                (Key::Char('j'), KeyModifiers::CONTROL) => {
                    self.textarea.insert_newline();
                }
                (Key::Char('l'), KeyModifiers::CONTROL) => {
                    self.set_block();
                }
                (Key::Tab, _) => return Some(Msg::FocusConversation),
                (Key::Esc, _) => return Some(Msg::AppClose),
                _ => {
                    let input = to_input(key);
                    self.textarea.input(input);
                }
            },
            Event::Paste(ref data) => {
                self.textarea.insert_str(data);
            }
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                ..
            }) => {
                if column >= self.area.x
                    && column < self.area.x + self.area.width
                    && row >= self.area.y
                    && row < self.area.y + self.area.height
                {
                    return Some(Msg::FocusInput);
                }
            }
            _ => {}
        }
        Some(Msg::None)
    }
}

impl MockComponent for Prompt {
    fn view(&mut self, frame: &mut Frame, area: Rect) {
        self.area = area;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::LightBlue))
            .title("Input");
        let inner = block.inner(area);
        let width = inner.width as usize;
        let mut display = Vec::new();
        for line in self.textarea.lines() {
            let wrapped = wrap(line, width);
            if wrapped.is_empty() {
                display.push(String::new());
            }
            for part in wrapped {
                display.push(part.into_owned());
            }
        }
        let text = display.join("\n");
        let mut paragraph = Paragraph::new(Text::raw(text))
            .block(block)
            .style(Style::default().fg(Color::LightBlue))
            .wrap(RtWrap { trim: false });

        // scroll to keep cursor visible
        let (_, _, cursor_y) = self.cursor_position(inner);
        let scroll_y = cursor_y.saturating_sub(inner.height.saturating_sub(1));
        paragraph = paragraph.scroll((scroll_y, 0));
        frame.render_widget(paragraph, area);
        let (cx, cy, _) = self.cursor_position(inner);
        frame.set_cursor_position(tuirealm::ratatui::prelude::Position { x: cx, y: cy });
    }

    fn query(&self, _attr: Attribute) -> Option<AttrValue> {
        None
    }

    fn attr(&mut self, _attr: Attribute, _value: AttrValue) {}

    fn state(&self) -> State {
        State::One(StateValue::String(self.textarea.lines().join("\n")))
    }

    fn perform(&mut self, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}
