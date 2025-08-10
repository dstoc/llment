use tuirealm::{
    Component, Frame, MockComponent, State,
    command::{Cmd, CmdResult},
    event::{Event, NoUserEvent},
    props::{AttrValue, Attribute, Props},
    ratatui::layout::Rect,
};

use super::history_item::{HistoryItemComponent, HistoryKind};

pub struct History {
    items: Vec<HistoryItemComponent>,
    props: Props,
}

impl History {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            props: Props::default(),
        }
    }

    pub fn push(&mut self, item: HistoryKind) {
        self.items.push(HistoryItemComponent::new(item));
    }
}

impl MockComponent for History {
    fn view(&mut self, frame: &mut Frame, area: Rect) {
        let mut y = area.y;
        for item in self.items.iter_mut() {
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            };
            item.view(frame, rect);
            y += 1;
        }
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
pub enum HistoryMsg {
    None,
}

impl Component<HistoryMsg, NoUserEvent> for History {
    fn on(&mut self, ev: Event<NoUserEvent>) -> Option<HistoryMsg> {
        for item in self.items.iter_mut() {
            item.on(ev.clone());
        }
        None
    }
}
