use tuirealm::{
    Component, Frame, MockComponent, State,
    command::{Cmd, CmdResult},
    event::{Event, NoUserEvent},
    props::{AttrValue, Attribute, Props},
    ratatui::layout::{Constraint, Direction, Layout, Rect},
};

use super::history::History;
use super::history_item::HistoryKind;
use super::input::{InputComponent, InputMsg};

pub struct Chat {
    pub history: History,
    input: InputComponent,
    props: Props,
}

impl Chat {
    pub fn new() -> Self {
        Self {
            history: History::new(),
            input: InputComponent::new(),
            props: Props::default(),
        }
    }
}

impl MockComponent for Chat {
    fn view(&mut self, frame: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(area);
        self.history.view(frame, layout[0]);
        self.input.view(frame, layout[1]);
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
pub enum ChatMsg {
    InputSubmitted(String),
    None,
}

impl Component<ChatMsg, NoUserEvent> for Chat {
    fn on(&mut self, ev: Event<NoUserEvent>) -> Option<ChatMsg> {
        if let Some(msg) = self.input.on(ev.clone()) {
            if let InputMsg::Submit(s) = msg {
                self.history.push(HistoryKind::User(s.clone()));
                return Some(ChatMsg::InputSubmitted(s));
            }
        }
        self.history.on(ev);
        None
    }
}
