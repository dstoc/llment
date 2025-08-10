use tuirealm::{
    Component, Frame, MockComponent, State,
    command::{Cmd, CmdResult},
    event::{Event, NoUserEvent},
    props::{AttrValue, Attribute, Props},
    ratatui::layout::Rect,
};

use super::chat::{Chat, ChatMsg};
use super::history_item::HistoryKind;

pub struct App {
    chat: Chat,
    props: Props,
}

impl App {
    pub fn new() -> Self {
        Self {
            chat: Chat::new(),
            props: Props::default(),
        }
    }
    pub fn push_assistant(&mut self, text: String) {
        self.chat.history.push(HistoryKind::Assistant(text));
    }
}

impl MockComponent for App {
    fn view(&mut self, frame: &mut Frame, area: Rect) {
        self.chat.view(frame, area);
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

#[derive(PartialEq)]
pub enum AppMsg {
    Send(String),
    None,
}

impl Component<AppMsg, NoUserEvent> for App {
    fn on(&mut self, ev: Event<NoUserEvent>) -> Option<AppMsg> {
        if let Some(msg) = self.chat.on(ev) {
            if let ChatMsg::InputSubmitted(s) = msg {
                return Some(AppMsg::Send(s));
            }
        }
        None
    }
}
