use crate::commands::SlashCommand;
use tuirealm::event::{Key, KeyEvent, KeyModifiers};
use tuirealm::ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, PartialEq, Eq)]
pub enum CommandPopupMsg {
    Navigate,
    Complete(SlashCommand),
}

/// Popup listing available slash commands.
#[derive(Debug)]
pub struct CommandPopup {
    pub matches: Vec<SlashCommand>,
    pub selected: usize,
    pub visible: bool,
}

impl CommandPopup {
    pub fn on_key(&mut self, key: KeyEvent) -> Option<CommandPopupMsg> {
        match key.code {
            Key::Up => {
                if self.selected == 0 {
                    self.selected = self.matches.len() - 1;
                } else {
                    self.selected -= 1;
                }
                Some(CommandPopupMsg::Navigate)
            }
            Key::Down => {
                self.selected = (self.selected + 1) % self.matches.len();
                Some(CommandPopupMsg::Navigate)
            }
            Key::Tab if key.modifiers == KeyModifiers::NONE => {
                Some(CommandPopupMsg::Complete(self.matches[self.selected]))
            }
            _ => None,
        }
    }

    pub fn view(&mut self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }
        let entries: Vec<String> = self
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
