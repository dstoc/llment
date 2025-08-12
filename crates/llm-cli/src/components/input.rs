use ratatui::layout::{Constraint, Direction, Layout};
use tui_textarea::{Input as TaInput, Key as TaKey, TextArea};
use tuirealm::command::{Cmd, CmdResult};
use tuirealm::event::{Key, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tuirealm::props::{AttrValue, Attribute};
use tuirealm::ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use tuirealm::{Component, Event, Frame, MockComponent, NoUserEvent, State, StateValue};
use unicode_width::UnicodeWidthStr;

use crate::{
    Msg,
    commands::{self, SlashCommand},
};

/// Multiline prompt input backed by [`tui_textarea`].
pub struct Prompt {
    textarea: TextArea<'static>,
    area: Rect,
    focused: bool,
    cmd: Option<CommandPopup>,
}

impl Prompt {
    fn new_textarea() -> TextArea<'static> {
        let mut ta = TextArea::default();
        ta.set_style(Style::default().fg(Color::LightBlue));
        ta.set_cursor_line_style(Style::default());
        ta
    }

    fn set_block(&mut self) {
        // Reapply block/style after clearing
        self.textarea = Self::new_textarea();
    }

    fn refresh_cmd_state(&mut self) {
        let text = self.textarea.lines().join("\n");
        if text.starts_with('/') && !text.contains('\n') {
            let prefix = &text[1..];
            let matches = commands::matches(prefix);
            if matches.is_empty() {
                self.cmd = None;
            } else {
                let selected = self
                    .cmd
                    .as_ref()
                    .map(|c| c.selected.min(matches.len() - 1))
                    .unwrap_or(0);
                self.cmd = Some(CommandPopup {
                    prefix: prefix.to_string(),
                    matches,
                    selected,
                    visible: true,
                });
            }
        } else {
            self.cmd = None;
        }
    }
}

struct CommandPopup {
    #[allow(dead_code)]
    prefix: String,
    matches: Vec<SlashCommand>,
    selected: usize,
    visible: bool,
}

impl Default for Prompt {
    fn default() -> Self {
        Self {
            textarea: Self::new_textarea(),
            area: Rect::default(),
            focused: false,
            cmd: None,
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
            Event::Keyboard(key) if self.focused => match (key.code, key.modifiers) {
                (Key::Char('j'), KeyModifiers::CONTROL) => {
                    self.textarea.insert_newline();
                    self.refresh_cmd_state();
                }
                (Key::Char('l'), KeyModifiers::CONTROL) => {
                    self.set_block();
                    self.cmd = None;
                }
                (Key::Enter, KeyModifiers::NONE) => {
                    let text = self.textarea.lines().join("\n");
                    let trimmed = text.trim().to_string();
                    let cmd = if let Some(state) = &self.cmd {
                        if state.visible {
                            Some(state.matches[state.selected])
                        } else if trimmed.starts_with('/') {
                            let name = &trimmed[1..];
                            let ms = commands::matches(name);
                            if ms.len() == 1 && ms[0].name() == name {
                                Some(ms[0])
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else if trimmed.starts_with('/') {
                        let name = &trimmed[1..];
                        let ms = commands::matches(name);
                        if ms.len() == 1 && ms[0].name() == name {
                            Some(ms[0])
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    self.set_block();
                    self.cmd = None;
                    if let Some(cmd) = cmd {
                        return Some(Msg::Slash(cmd));
                    }
                    if trimmed.is_empty() {
                        return Some(Msg::None);
                    }
                    return Some(Msg::Submit(trimmed));
                }
                (Key::Up, _) => {
                    if let Some(state) = &mut self.cmd {
                        if state.visible {
                            if state.selected == 0 {
                                state.selected = state.matches.len() - 1;
                            } else {
                                state.selected -= 1;
                            }
                            return Some(Msg::None);
                        }
                    }
                    let input = to_input(key);
                    self.textarea.input(input);
                    self.refresh_cmd_state();
                }
                (Key::Down, _) => {
                    if let Some(state) = &mut self.cmd {
                        if state.visible {
                            state.selected = (state.selected + 1) % state.matches.len();
                            return Some(Msg::None);
                        }
                    }
                    let input = to_input(key);
                    self.textarea.input(input);
                    self.refresh_cmd_state();
                }
                (Key::Tab, KeyModifiers::NONE) => {
                    if let Some(state) = &mut self.cmd {
                        if state.visible {
                            let cmd = state.matches[state.selected];
                            self.set_block();
                            self.textarea.insert_str(&format!("/{}", cmd.name()));
                            self.cmd = None;
                            self.refresh_cmd_state();
                            return Some(Msg::None);
                        }
                    }
                    let input = to_input(key);
                    self.textarea.input(input);
                    self.refresh_cmd_state();
                }
                (Key::Esc, _) => return Some(Msg::AppClose),
                _ => {
                    let input = to_input(key);
                    self.textarea.input(input);
                    self.refresh_cmd_state();
                }
            },
            Event::Paste(ref data) if self.focused => {
                self.textarea.insert_str(data);
                self.refresh_cmd_state();
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
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(2), Constraint::Min(0)].as_ref())
            .split(area);

        frame.render_widget(Paragraph::new("> "), chunks[0]);
        frame.render_widget(&self.textarea, chunks[1]);
        self.area = chunks[1];

        if let Some(state) = &self.cmd {
            if state.visible {
                let entries: Vec<String> = state
                    .matches
                    .iter()
                    .map(|c| format!("/{} - {}", c.name(), c.description()))
                    .collect();
                let popup_width = entries
                    .iter()
                    .map(|s| s.as_str().width())
                    .max()
                    .unwrap_or(0) as u16
                    + 2;
                let items: Vec<ListItem> = entries.into_iter().map(ListItem::new).collect();
                let popup_height = items.len() as u16 + 2;
                let popup_area = Rect {
                    x: chunks[1].x,
                    y: chunks[1].y.saturating_sub(popup_height),
                    width: popup_width,
                    height: popup_height,
                };
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL))
                    .highlight_style(Style::default().bg(Color::Blue));
                let mut list_state = ListState::default();
                list_state.select(Some(state.selected));
                frame.render_widget(Clear, popup_area);
                frame.render_stateful_widget(list, popup_area, &mut list_state);
            }
        }
    }

    fn query(&self, attr: Attribute) -> Option<AttrValue> {
        match attr {
            Attribute::Height => {
                let lines = self.textarea.lines().len().max(1);
                Some(AttrValue::Length(lines))
            }
            _ => None,
        }
    }

    fn attr(&mut self, attr: Attribute, value: AttrValue) {
        if let Attribute::Focus = attr {
            if let AttrValue::Flag(f) = value {
                self.focused = f;
            }
        }
    }

    fn state(&self) -> State {
        State::One(StateValue::String(self.textarea.lines().join("\n")))
    }

    fn perform(&mut self, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}
