use tuirealm::event::{Key, KeyEvent, KeyModifiers};
use tuirealm::{
    Component, Frame, MockComponent, State,
    command::{Cmd, CmdResult},
    event::Event,
    props::{AttrValue, Attribute, Props},
    ratatui::{
        layout::Rect,
        widgets::{Block, Borders, Paragraph as TuiParagraph},
    },
};

use crate::event::ChatEvent;

pub struct InputComponent {
    value: String,
    props: Props,
}

impl InputComponent {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            props: Props::default(),
        }
    }
}

impl MockComponent for InputComponent {
    fn view(&mut self, frame: &mut Frame, area: Rect) {
        let widget =
            TuiParagraph::new(self.value.clone()).block(Block::default().borders(Borders::ALL));
        frame.render_widget(widget, area);
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
pub enum InputMsg {
    Submit(String),
    Exit,
    None,
}

impl Component<InputMsg, ChatEvent> for InputComponent {
    fn on(&mut self, ev: Event<ChatEvent>) -> Option<InputMsg> {
        if let Event::Keyboard(KeyEvent {
            code, modifiers, ..
        }) = ev
        {
            match (code, modifiers) {
                (Key::Enter, _) => {
                    let val = self.value.clone();
                    self.value.clear();
                    return Some(InputMsg::Submit(val));
                }
                (Key::Char('d'), KeyModifiers::CONTROL) => {
                    return Some(InputMsg::Exit);
                }
                (Key::Char(c), _) => self.value.push(c),
                (Key::Backspace, _) => {
                    self.value.pop();
                }
                _ => {}
            }
        }
        None
    }
}
