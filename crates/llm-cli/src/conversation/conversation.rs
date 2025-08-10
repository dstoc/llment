use tuirealm::event::{Key, KeyEvent, MouseButton, MouseEventKind};
use tuirealm::ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders},
};
use tuirealm::{Component, Event, MockComponent, NoUserEvent};

use crate::Msg;

use super::{AssistantBlock, Node, ThoughtStep, ToolStep, UserBubble, node::ConvNode};

pub struct Conversation {
    pub(crate) items: Vec<Node>,
    pub(crate) selected: usize,
    pub(crate) scroll: u16,
    pub(crate) layout: Vec<(u16, u16)>,
    pub(crate) width: u16,
    pub(crate) viewport: u16,
    pub(crate) dirty: bool,
    pub(crate) area: Rect,
    pub(crate) focused: bool,
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            items: sample_items(),
            selected: 0,
            scroll: 0,
            layout: Vec::new(),
            width: 0,
            viewport: 0,
            dirty: true,
            area: Rect::default(),
            focused: false,
        }
    }
}

fn sample_items() -> Vec<Node> {
    vec![
        Node::User(UserBubble::new(
            "Hello! I'm testing the conversation view. This message should be long enough to wrap and require scrolling.".into(),
        )),
        Node::Assistant(AssistantBlock::new(
            false,
            vec![
                Node::Thought(ThoughtStep::new("Analyzing the request".into())),
                Node::Tool(ToolStep::new(
                    "search".into(),
                    "{\"query\":\"scrolling\"}".into(),
                    "{\"answer\":42}".into(),
                    true,
                )),
            ],
            "Here's an example response after some thinking and a tool call.".into(),
        )),
        Node::User(UserBubble::new(
            "Can you show more details? Another long line is helpful.".into(),
        )),
        Node::Assistant(AssistantBlock::new(
            true,
            vec![
                Node::Thought(ThoughtStep::new("Another thought".into())),
                Node::Tool(ToolStep::new(
                    "math".into(),
                    "1+1".into(),
                    "2".into(),
                    true,
                )),
            ],
            "Yes, there's more to see.".into(),
        )),
        Node::User(UserBubble::new(
            "This is a final message to ensure scrolling works properly.".into(),
        )),
        Node::Assistant(AssistantBlock::new(
            false,
            vec![Node::Thought(ThoughtStep::new("Wrapping things up".into()))],
            "All done!".into(),
        )),
    ]
}

impl Conversation {
    pub(crate) fn ensure_layout(&mut self, width: u16) {
        if self.width != width || self.dirty {
            self.width = width;
            self.layout.clear();
            let mut pos = 0;
            for item in self.items.iter_mut() {
                let h = item.height(width);
                self.layout.push((pos, h));
                pos += h;
            }
            self.dirty = false;
        }
    }

    pub(crate) fn total_height(&self) -> u16 {
        self.layout.last().map(|(s, h)| s + h).unwrap_or(0)
    }

    pub(crate) fn ensure_visible(&mut self) {
        if self.layout.is_empty() {
            return;
        }
        let (start, h) = self.layout[self.selected];
        let end = start + h;
        if start < self.scroll {
            self.scroll = start;
        } else if end > self.scroll + self.viewport {
            self.scroll = end.saturating_sub(self.viewport);
        }
    }
}

impl MockComponent for Conversation {
    fn view(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightBlue))
            .title(Span::styled(
                "Conversation",
                Style::default().fg(Color::LightBlue),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        self.viewport = inner.height;
        self.area = inner;
        self.ensure_layout(inner.width);

        for (idx, item) in self.items.iter_mut().enumerate() {
            let (start, h) = self.layout[idx];
            if start + h <= self.scroll {
                continue;
            }
            if start >= self.scroll + self.viewport {
                break;
            }
            let offset = self.scroll.saturating_sub(start);
            let y = inner.y + start.saturating_sub(self.scroll);
            let remaining = self.viewport.saturating_sub(y - inner.y);
            let max_height = remaining.min(h - offset);
            let rect = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: max_height,
            };
            item.render(frame, rect, idx == self.selected, offset, max_height);
        }
    }

    fn query(&self, _attr: tuirealm::Attribute) -> Option<tuirealm::AttrValue> {
        None
    }

    fn attr(&mut self, attr: tuirealm::Attribute, value: tuirealm::AttrValue) {
        if let tuirealm::Attribute::Focus = attr {
            if let tuirealm::AttrValue::Flag(f) = value {
                self.focused = f;
            }
        }
    }

    fn state(&self) -> tuirealm::State {
        tuirealm::State::None
    }

    fn perform(&mut self, _cmd: tuirealm::command::Cmd) -> tuirealm::command::CmdResult {
        tuirealm::command::CmdResult::None
    }
}

impl Component<Msg, NoUserEvent> for Conversation {
    fn on(&mut self, ev: Event<NoUserEvent>) -> Option<Msg> {
        match ev {
            Event::Keyboard(KeyEvent { code, .. }) if self.focused => match code {
                Key::Down => {
                    if !self.items[self.selected].on_key(Key::Down) {
                        if self.selected + 1 < self.items.len() {
                            self.selected += 1;
                        }
                    }
                    self.ensure_visible();
                }
                Key::Up => {
                    if !self.items[self.selected].on_key(Key::Up) {
                        if self.selected > 0 {
                            self.selected -= 1;
                        }
                    }
                    self.ensure_visible();
                }
                Key::PageDown => {
                    self.scroll = self.scroll.saturating_add(self.viewport);
                    let max = self.total_height().saturating_sub(self.viewport);
                    if self.scroll > max {
                        self.scroll = max;
                    }
                }
                Key::PageUp => {
                    self.scroll = self.scroll.saturating_sub(self.viewport);
                }
                Key::Home => {
                    self.selected = 0;
                    self.scroll = 0;
                }
                Key::End => {
                    if !self.items.is_empty() {
                        self.selected = self.items.len() - 1;
                        self.scroll = self.total_height().saturating_sub(self.viewport);
                    }
                }
                Key::Enter => {
                    self.items[self.selected].activate();
                    self.dirty = true;
                    self.ensure_layout(self.width);
                    self.ensure_visible();
                }
                Key::Tab => return Some(Msg::FocusInput),
                Key::Esc => return Some(Msg::AppClose),
                _ => {}
            },
            Event::Keyboard(_) => {}
            Event::Mouse(me) => {
                if me.column >= self.area.x
                    && me.column < self.area.x + self.area.width
                    && me.row >= self.area.y
                    && me.row < self.area.y + self.area.height
                {
                    match me.kind {
                        MouseEventKind::ScrollUp => {
                            self.scroll = self.scroll.saturating_sub(1);
                            return Some(Msg::FocusConversation);
                        }
                        MouseEventKind::ScrollDown => {
                            let max = self.total_height().saturating_sub(self.viewport);
                            self.scroll = (self.scroll + 1).min(max);
                            return Some(Msg::FocusConversation);
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            let line = self.scroll + (me.row - self.area.y) as u16;
                            let mut target: Option<(usize, u16)> = None;
                            for (i, (start, h)) in self.layout.iter().enumerate() {
                                if line >= *start && line < *start + *h {
                                    target = Some((i, *start));
                                    break;
                                }
                            }
                            if let Some((idx, start)) = target {
                                self.selected = idx;
                                let rel = line - start;
                                self.items[idx].click(rel);
                                self.dirty = true;
                                self.ensure_layout(self.width);
                                self.ensure_visible();
                            }
                            return Some(Msg::FocusConversation);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Some(Msg::None)
    }
}
