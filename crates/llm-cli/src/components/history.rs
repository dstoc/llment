use tuirealm::{
    Component, Frame, MockComponent, State,
    command::{Cmd, CmdResult},
    event::Event,
    props::{AttrValue, Attribute, Props},
    ratatui::layout::Rect,
};

use crate::event::ChatEvent;

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

    pub fn apply_chunk(&mut self, chunk: llm_core::ResponseChunk) {
        if let Some(thinking) = chunk.message.thinking {
            match self.items.last_mut() {
                Some(item) => match item.kind_mut() {
                    HistoryKind::Thinking { .. } => item.push_text(&thinking),
                    _ => self
                        .items
                        .push(HistoryItemComponent::new(HistoryKind::Thinking {
                            content: thinking,
                            collapsed: true,
                        })),
                },
                None => self
                    .items
                    .push(HistoryItemComponent::new(HistoryKind::Thinking {
                        content: thinking,
                        collapsed: true,
                    })),
            }
        }

        if !chunk.message.content.is_empty() {
            match self.items.last_mut() {
                Some(item) => match item.kind_mut() {
                    HistoryKind::Assistant(_) => item.push_text(&chunk.message.content),
                    _ => self
                        .items
                        .push(HistoryItemComponent::new(HistoryKind::Assistant(
                            chunk.message.content,
                        ))),
                },
                None => self
                    .items
                    .push(HistoryItemComponent::new(HistoryKind::Assistant(
                        chunk.message.content,
                    ))),
            }
        }
    }

    pub fn toggle_last_thinking(&mut self) {
        for item in self.items.iter_mut().rev() {
            if matches!(item.kind(), HistoryKind::Thinking { .. }) {
                item.toggle();
                break;
            }
        }
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

impl Component<HistoryMsg, ChatEvent> for History {
    fn on(&mut self, ev: Event<ChatEvent>) -> Option<HistoryMsg> {
        for item in self.items.iter_mut() {
            item.on(ev.clone());
        }
        None
    }
}
