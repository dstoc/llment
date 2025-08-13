use ratatui::layout::{Constraint, Direction, Layout};
use tui_textarea::{Input as TaInput, Key as TaKey, TextArea};
use tuirealm::command::{Cmd, CmdResult};
use tuirealm::event::{Key, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tuirealm::props::{AttrValue, Attribute, PropValue};
use tuirealm::ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
};
use tuirealm::{Component, Event, Frame, MockComponent, NoUserEvent, State, StateValue};
use unicode_width::UnicodeWidthStr;

use crate::{Msg, commands};

use super::{
    command_popup::{CommandPopup, CommandPopupMsg},
    param_popup::{ParamPopup, ParamPopupMsg},
};
use clap::ValueEnum;
use llm::{self, Provider};
use std::collections::HashMap;
use tokio::runtime::Handle;

/// Multiline prompt input backed by [`tui_textarea`].
pub struct Prompt {
    textarea: TextArea<'static>,
    area: Rect,
    focused: bool,
    cmd: Option<CommandPopup>,
    param: Option<ParamPopup>,
    models: Vec<String>,
    current_provider: Provider,
    model_cache: HashMap<Provider, Vec<String>>,
    host: String,
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

    pub fn with_models(provider: Provider, host: String, models: Vec<String>) -> Self {
        let mut cache = HashMap::new();
        cache.insert(provider, models.clone());
        Self {
            textarea: Self::new_textarea(),
            area: Rect::default(),
            focused: false,
            cmd: None,
            param: None,
            models,
            current_provider: provider,
            model_cache: cache,
            host,
        }
    }

    fn provider_param_matches(&mut self, prefix: &str) -> Vec<String> {
        if let Some((prov, model_prefix)) = prefix.split_once(' ') {
            if let Ok(provider) = Provider::from_str(prov, true) {
                let models = self.model_cache.entry(provider).or_insert_with(|| {
                    let client = llm::client_from(provider, &self.host).expect("client");
                    Handle::current()
                        .block_on(client.list_models())
                        .unwrap_or_default()
                });
                models
                    .iter()
                    .filter(|m| m.starts_with(model_prefix))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Provider::value_variants()
                .iter()
                .map(|p| p.to_possible_value().unwrap().get_name().to_string())
                .filter(|p| p.starts_with(prefix))
                .collect()
        }
    }

    fn refresh_cmd_state(&mut self) {
        let text = self.textarea.lines().join("\n");
        if text.starts_with('/') && !text.contains('\n') {
            let rest = &text[1..];
            if let Some((name, param)) = rest.split_once(' ') {
                let matches = commands::matches(name);
                if matches.len() == 1 && matches[0].name() == name {
                    let cmd = matches[0];
                    let params = match cmd {
                        commands::SlashCommand::Model if !param.contains(' ') => {
                            commands::param_matches(cmd, param, &self.models)
                        }
                        commands::SlashCommand::Provider => self.provider_param_matches(param),
                        _ => Vec::new(),
                    };
                    if params.is_empty() {
                        self.param = None;
                    } else {
                        let selected = self
                            .param
                            .as_ref()
                            .map(|p| p.selected.min(params.len() - 1))
                            .unwrap_or(0);
                        let offset = format!("/{} ", name).width() as u16;
                        self.param = Some(ParamPopup {
                            cmd,
                            matches: params,
                            selected,
                            visible: true,
                            offset,
                        });
                    }
                    self.cmd = None;
                    return;
                }
            }
            let matches = commands::matches(rest);
            if matches.is_empty() {
                self.cmd = None;
            } else {
                let selected = self
                    .cmd
                    .as_ref()
                    .map(|c| c.selected.min(matches.len() - 1))
                    .unwrap_or(0);
                self.cmd = Some(CommandPopup {
                    matches,
                    selected,
                    visible: true,
                });
            }
            self.param = None;
        } else {
            self.cmd = None;
            self.param = None;
        }
    }
}

impl Default for Prompt {
    fn default() -> Self {
        Self {
            textarea: Self::new_textarea(),
            area: Rect::default(),
            focused: false,
            cmd: None,
            param: None,
            models: Vec::new(),
            current_provider: Provider::Ollama,
            model_cache: HashMap::new(),
            host: String::new(),
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
            Event::Keyboard(key) if self.focused => {
                if let Some(popup) = &mut self.param {
                    if popup.visible {
                        if let Some(msg) = popup.on_key(key.clone()) {
                            match msg {
                                ParamPopupMsg::Navigate => {}
                                ParamPopupMsg::Complete { cmd, param } => {
                                    self.set_block();
                                    self.textarea
                                        .insert_str(&format!("/{} {}", cmd.name(), param));
                                    self.param = None;
                                    return Some(Msg::None);
                                }
                                ParamPopupMsg::Submit { cmd, param } => {
                                    self.set_block();
                                    self.cmd = None;
                                    self.param = None;
                                    return Some(Msg::Slash(cmd, Some(param)));
                                }
                            }
                        }
                    }
                }
                if let Some(popup) = &mut self.cmd {
                    if popup.visible {
                        if let Some(msg) = popup.on_key(key.clone()) {
                            match msg {
                                CommandPopupMsg::Navigate => {}
                                CommandPopupMsg::Complete(cmd) => {
                                    self.set_block();
                                    if cmd.takes_param() {
                                        self.textarea.insert_str(&format!("/{} ", cmd.name()));
                                    } else {
                                        self.textarea.insert_str(&format!("/{}", cmd.name()));
                                    }
                                    self.cmd = None;
                                    self.refresh_cmd_state();
                                    return Some(Msg::None);
                                }
                            }
                        }
                    }
                }
                match (key.code, key.modifiers) {
                    (Key::Char('j'), KeyModifiers::CONTROL) => {
                        self.textarea.insert_newline();
                        self.refresh_cmd_state();
                    }
                    (Key::Char('l'), KeyModifiers::CONTROL) => {
                        self.set_block();
                        self.cmd = None;
                        self.param = None;
                    }
                    (Key::Enter, KeyModifiers::NONE) => {
                        let text = self.textarea.lines().join("\n");
                        let trimmed = text.trim().to_string();
                        self.set_block();
                        self.cmd = None;
                        self.param = None;
                        if let Some((cmd, param)) = commands::parse(&trimmed) {
                            return Some(Msg::Slash(cmd, param));
                        }
                        if trimmed.is_empty() {
                            return Some(Msg::None);
                        }
                        return Some(Msg::Submit(trimmed));
                    }
                    (Key::Esc, _) => return Some(Msg::AppClose),
                    _ => {
                        let input = to_input(key);
                        self.textarea.input(input);
                        self.refresh_cmd_state();
                    }
                }
                return Some(Msg::None);
            }
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

        if let Some(state) = &mut self.param {
            state.view(frame, chunks[1]);
        } else if let Some(state) = &mut self.cmd {
            state.view(frame, chunks[1]);
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
        match attr {
            Attribute::Focus => {
                if let AttrValue::Flag(f) = value {
                    self.focused = f;
                }
            }
            Attribute::Text => {
                if let AttrValue::String(s) = value {
                    self.set_block();
                    self.textarea.insert_str(&s);
                    self.refresh_cmd_state();
                }
            }
            Attribute::Custom("provider") => {
                if let AttrValue::String(s) = value {
                    if let Ok(p) = Provider::from_str(&s, true) {
                        self.current_provider = p;
                    }
                }
            }
            Attribute::Custom("models") => {
                if let AttrValue::Payload(p) = value {
                    let vec = p.unwrap_vec();
                    let models: Vec<String> = vec
                        .into_iter()
                        .filter_map(|v| match v {
                            PropValue::Str(s) => Some(s),
                            _ => None,
                        })
                        .collect();
                    self.models = models.clone();
                    self.model_cache.insert(self.current_provider, models);
                }
            }
            _ => {}
        }
    }

    fn state(&self) -> State {
        State::One(StateValue::String(self.textarea.lines().join("\n")))
    }

    fn perform(&mut self, _cmd: Cmd) -> CmdResult {
        CmdResult::None
    }
}
