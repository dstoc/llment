use crate::component::Component;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use futures_signals::signal::Mutable;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
};
use tui_textarea::{Input as TaInput, Key as TaKey, TextArea};

use super::completion::{Command, CommandRouter, CompletionPopup, CompletionResult};

/// Multiline prompt input backed by [`tui_textarea`].
pub struct Prompt {
    pub model: PromptModel,
    textarea: TextArea<'static>,
    area: Rect,
    focused: bool,
    completion: CompletionPopup,
    router: CommandRouter,
    needs_update: Mutable<bool>,
}

#[derive(Default)]
pub struct PromptModel {
    pub submitted_prompt: Mutable<String>,
    pub needs_redraw: Mutable<bool>,
    pub needs_update: Mutable<bool>,
}

impl Prompt {
    pub fn new(model: PromptModel, commands: Vec<Box<dyn Command>>) -> Self {
        Self {
            model,
            textarea: Self::new_textarea(),
            area: Rect::default(),
            focused: false,
            completion: CompletionPopup {
                result: None,
                selected: 0,
            },
            router: CommandRouter::new(commands),
            needs_update: Mutable::new(false),
        }
    }

    pub fn height(&self) -> u16 {
        self.textarea.lines().len().max(1) as u16
    }

    pub fn set_prompt(&mut self, prompt: String) {
        self.reset();
        self.textarea.insert_str(prompt);
        self.model.needs_redraw.set(true);
    }

    fn new_textarea() -> TextArea<'static> {
        let mut ta = TextArea::default();
        ta.set_style(Style::default().fg(Color::LightBlue));
        ta.set_cursor_line_style(Style::default());
        ta
    }

    fn update_completion(&mut self) {
        let text = self.textarea.lines().join("\n");
        let result = self.router.update(text.as_str());
        self.completion.result = Some(result);
        if let Some(CompletionResult::Loading { at: _, done }) = &mut self.completion.result {
            let (tx, closed) = tokio::sync::oneshot::channel();
            drop(tx);
            let done = std::mem::replace(done, closed);
            let model_needs_update = self.model.needs_update.clone();
            let self_needs_update = self.needs_update.clone();
            tokio::spawn(async move {
                if done.await.is_ok() {
                    model_needs_update.set(true);
                    self_needs_update.set(true);
                }
            });
        }
    }

    fn try_commit_completion(&mut self) -> bool {
        self.router.commit().is_ok()
    }

    fn reset(&mut self) {
        self.textarea = Prompt::new_textarea();
        self.completion.result = None;
        self.completion.selected = 0;
    }
}

fn to_input(ev: KeyEvent) -> TaInput {
    let key = match ev.code {
        KeyCode::Backspace => TaKey::Backspace,
        KeyCode::Enter => TaKey::Enter,
        KeyCode::Left => TaKey::Left,
        KeyCode::Right => TaKey::Right,
        KeyCode::Up => TaKey::Up,
        KeyCode::Down => TaKey::Down,
        KeyCode::Home => TaKey::Home,
        KeyCode::End => TaKey::End,
        KeyCode::Tab => TaKey::Tab,
        KeyCode::Delete => TaKey::Delete,
        KeyCode::Char(c) => TaKey::Char(c),
        _ => TaKey::Null,
    };
    TaInput {
        key,
        ctrl: ev.modifiers.contains(KeyModifiers::CONTROL),
        alt: ev.modifiers.contains(KeyModifiers::ALT),
        shift: ev.modifiers.contains(KeyModifiers::SHIFT),
    }
}

impl Component for Prompt {
    fn update(&mut self) {
        if self.needs_update.get() {
            self.needs_update.set(false);
            self.update_completion();
        }
    }
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(2), Constraint::Min(0)].as_ref())
            .split(area);

        frame.render_widget(Paragraph::new("> "), chunks[0]);
        frame.render_widget(&self.textarea, chunks[1]);
        self.area = chunks[1];

        if self.completion.visible() {
            self.completion.view(frame, chunks[1]);
        }
    }
    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) => {
                if self.completion.visible() {
                    if let Some(action) = self.completion.on_key(key) {
                        match action {
                            super::completion::CompletionPopupAction::Complete {
                                at,
                                str,
                                commit,
                            } => {
                                if !str.is_empty() {
                                    let prefix: String =
                                        self.textarea.lines().join("\n").chars().take(at).collect();
                                    self.textarea = Prompt::new_textarea();
                                    self.textarea.insert_str(&format!("{}{}", prefix, str));
                                    self.update_completion();
                                    self.model.needs_redraw.set(true);
                                }
                                if commit {
                                    if self.try_commit_completion() {
                                        self.reset();
                                    }
                                    self.model.needs_redraw.set(true);
                                }
                                return;
                            }
                            super::completion::CompletionPopupAction::Redraw => {
                                self.model.needs_redraw.set(true);
                                return;
                            }
                        }
                    }
                }
                if key.code == KeyCode::Enter {
                    let text = self.textarea.lines().join("\n");
                    let trimmed = text.trim().to_string();
                    self.textarea = Self::new_textarea();
                    if !trimmed.is_empty() {
                        self.model.submitted_prompt.set(trimmed);
                    }
                    self.model.needs_redraw.set(true);
                } else {
                    let input = to_input(key);
                    self.textarea.input(input);
                    self.model.needs_redraw.set(true);
                    self.update_completion();
                }
            }
            Event::Paste(ref data) if self.focused => {
                self.textarea.insert_str(data);
                self.model.needs_redraw.set(true);
                self.update_completion();
            }

            _ => (),
        }
    }
}
