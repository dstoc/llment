use tuirealm::{
    Component, Frame, MockComponent, State,
    command::{Cmd, CmdResult},
    event::{Event, NoUserEvent},
    props::{AttrValue, Attribute, Props},
    ratatui::{layout::Rect, widgets::Paragraph as TuiParagraph},
};

#[derive(Clone)]
pub enum HistoryKind {
    User(String),
    Assistant(String),
    Thinking { content: String, collapsed: bool },
}

pub struct HistoryItemComponent {
    kind: HistoryKind,
    props: Props,
}

impl HistoryItemComponent {
    pub fn new(kind: HistoryKind) -> Self {
        Self {
            kind,
            props: Props::default(),
        }
    }
}

impl MockComponent for HistoryItemComponent {
    fn view(&mut self, frame: &mut Frame, area: Rect) {
        let text = match &self.kind {
            HistoryKind::User(t) => t.clone(),
            HistoryKind::Assistant(t) => t.clone(),
            HistoryKind::Thinking { content, collapsed } => {
                if *collapsed {
                    "…".to_string()
                } else {
                    content.clone()
                }
            }
        };
        let widget = TuiParagraph::new(text);
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
pub enum HistoryItemMsg {
    None,
}

impl Component<HistoryItemMsg, NoUserEvent> for HistoryItemComponent {
    fn on(&mut self, _ev: Event<NoUserEvent>) -> Option<HistoryItemMsg> {
        None
    }
}
