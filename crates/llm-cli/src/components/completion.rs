use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};
use tokio::sync::oneshot;
use unicode_width::UnicodeWidthStr;

pub struct CompletionPopup {
    pub result: Option<CompletionResult>,
    pub selected: usize,
}

pub enum CompletionPopupAction {
    Complete {
        at: usize,
        str: String,
        commit: bool,
    },
    Redraw,
}

impl CompletionPopup {
    pub fn visible(&self) -> bool {
        if let Some(result) = &self.result {
            match result {
                CompletionResult::Options { options, .. } => !options.is_empty(),
                CompletionResult::Loading { .. } => true,
                _ => false,
            }
        } else {
            false
        }
    }
    pub fn on_key(&mut self, key: KeyEvent) -> Option<CompletionPopupAction> {
        if let Some(CompletionResult::Options { at, options }) = &self.result {
            match key.code {
                KeyCode::Up => {
                    if self.selected == 0 {
                        self.selected = options.len() - 1;
                    } else {
                        self.selected -= 1;
                    }
                    Some(CompletionPopupAction::Redraw)
                }
                KeyCode::Down => {
                    self.selected = (self.selected + 1) % options.len();
                    Some(CompletionPopupAction::Redraw)
                }
                KeyCode::Tab if key.modifiers == KeyModifiers::NONE => {
                    let str = if options.is_empty() {
                        "".to_string()
                    } else {
                        options[self.selected % options.len()].str.clone()
                    };

                    Some(CompletionPopupAction::Complete {
                        at: *at,
                        str,
                        commit: false,
                    })
                }
                KeyCode::Enter => {
                    let str = if options.is_empty() {
                        "".to_string()
                    } else {
                        options[self.selected % options.len()].str.clone()
                    };

                    Some(CompletionPopupAction::Complete {
                        at: *at,
                        str,
                        commit: true,
                    })
                }
                _ => None,
            }
        } else {
            None
        }
    }

    pub fn view(&mut self, frame: &mut Frame, area: Rect) {
        if !self.visible() {
            return;
        }
        let entries: Vec<String> = if let Some(result) = &self.result {
            match result {
                CompletionResult::Options { at: _, options } => options
                    .iter()
                    .map(|c| format!("{} - {}", c.name, c.description))
                    .collect(),
                CompletionResult::Loading { at: _, done: _ } => vec!["Loading...".to_string()],
                _ => vec!["???".to_string()],
            }
        } else {
            vec![]
        };
        let popup_width = entries
            .iter()
            .map(|s| s.as_str().width())
            .max()
            .unwrap_or(0) as u16
            + 2;
        let items: Vec<ListItem> = entries.into_iter().map(ListItem::new).collect();
        let popup_height = items.len() as u16 + 2;
        let popup_area = Rect {
            x: area.x,
            y: area.y.saturating_sub(popup_height),
            width: popup_width,
            height: popup_height,
        };
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::Blue));
        let mut list_state = ListState::default();
        list_state.select(Some(self.selected));
        frame.render_widget(Clear, popup_area);
        frame.render_stateful_widget(list, popup_area, &mut list_state);
    }
}

#[derive(Debug)]
pub struct Completion {
    pub name: String,
    pub description: String,
    pub str: String,
}

pub enum CompletionResult {
    /// the index where the completion options apply from
    Options { at: usize, options: Vec<Completion> },
    Loading {
        at: usize,
        done: oneshot::Receiver<()>,
    },
    /// the index where the error begins
    Invalid { at: usize },
}

pub trait Command {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn has_params(&self) -> bool {
        false
    }
    fn instance(&self) -> Box<dyn CommandInstance>;
}

/// Instances are used in a particular completion context. They can
/// cache the completion state and hold on to task handles.
pub trait CommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult;
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>>;
}

pub struct CommandRouter {
    commands: Vec<Box<dyn Command>>,
    active: Option<(String, Box<dyn CommandInstance>)>,
}

impl CommandRouter {
    pub fn new(commands: Vec<Box<dyn Command>>) -> Self {
        Self {
            commands,
            active: None,
        }
    }

    fn complete_names(&self, typed: &str) -> CompletionResult {
        let mut options: Vec<Completion> = self
            .commands
            .iter()
            .filter(|c| c.name().starts_with(typed))
            .map(|c| Completion {
                name: c.name().to_string(),
                description: c.description().to_string(),
                str: format!("{}{}", c.name(), if c.has_params() { " " } else { "" }),
            })
            .collect();
        options.sort_by(|a, b| a.name.cmp(&b.name));
        // completions apply after the '/'
        CompletionResult::Options { at: 1, options }
    }

    fn find_command(&self, token: &str) -> Option<&dyn Command> {
        self.commands
            .iter()
            .map(|c| &**c)
            .find(|c| c.name() == token)
    }

    fn offset(res: CompletionResult, by: usize) -> CompletionResult {
        match res {
            CompletionResult::Options { at, options } => CompletionResult::Options {
                at: at + by,
                options,
            },
            CompletionResult::Loading { at, done } => {
                CompletionResult::Loading { at: at + by, done }
            }
            CompletionResult::Invalid { at } => CompletionResult::Invalid { at: at + by },
        }
    }
}

impl CommandInstance for CommandRouter {
    fn update(&mut self, input: &str) -> CompletionResult {
        if !input.starts_with('/') {
            self.active = None;
            return CompletionResult::Invalid { at: 0 };
        }

        let rest = &input[1..];
        let (token, after_opt) = match rest.find(' ') {
            None => (rest, None),
            Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        };

        if let Some(cmd) = self.find_command(token) {
            if match &self.active {
                Some((active_name, _)) if active_name == cmd.name() => false,
                _ => true,
            } {
                self.active = Some((cmd.name().to_string(), cmd.instance()));
            }
        } else {
            self.active = None;
        }

        if after_opt.is_none() {
            return self.complete_names(token);
        }

        if let Some((name, _)) = &self.active {
            // Delegate to the active instance with the substring after "/name "
            let prefix = 1 + name.len() + 1; // "/{name} "
            let inner_res = self
                .active
                .as_mut()
                .expect("active set or already present")
                .1
                .update(after_opt.unwrap_or_default());

            Self::offset(inner_res, prefix)
        } else {
            CompletionResult::Invalid { at: 1 }
        }
    }

    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.active
            .as_ref()
            .map(|(_, inst)| inst.commit())
            .unwrap_or_else(|| Err("no active command instance to commit".into()))
    }
}
