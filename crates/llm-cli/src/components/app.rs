use tuirealm::{
    Component, Frame, MockComponent, State,
    command::{Cmd, CmdResult},
    event::Event,
    props::{AttrValue, Attribute, Props},
    ratatui::layout::Rect,
};

use crate::event::ChatEvent;

use super::chat::{Chat, ChatMsg};

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
    Quit,
    None,
}

impl Component<AppMsg, ChatEvent> for App {
    fn on(&mut self, ev: Event<ChatEvent>) -> Option<AppMsg> {
        if let Some(msg) = self.chat.on(ev) {
            match msg {
                ChatMsg::InputSubmitted(s) => return Some(AppMsg::Send(s)),
                ChatMsg::Exit => return Some(AppMsg::Quit),
                ChatMsg::None => {}
            }
        }
        None
    }
}
