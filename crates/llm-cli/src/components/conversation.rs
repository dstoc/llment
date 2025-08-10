use textwrap::wrap;
use tui_realm_stdlib::Textarea;
use tuirealm::command::{Cmd, CmdResult, Direction, Position};
use tuirealm::event::{Key, KeyEvent};
use tuirealm::props::{Alignment, BorderType, Borders, Color, TextSpan};
use tuirealm::ratatui::Frame;
use tuirealm::{AttrValue, Attribute, Component, Event, MockComponent, NoUserEvent, State};

use crate::Msg;

pub enum ConversationItem {
    User(String),
    Assistant {
        working: WorkingBlock,
        response: String,
    },
}

pub struct WorkingBlock {
    steps: Vec<WorkingItem>,
    collapsed: bool,
}

pub enum WorkingItem {
    Thoughts(String),
    Tool {
        name: String,
        args: String,
        result: String,
        collapsed: bool,
    },
}

#[derive(Clone, Copy)]
enum LineTarget {
    None,
    Working { item: usize },
    Tool { item: usize, step: usize },
}

pub struct Conversation {
    component: Textarea,
    items: Vec<ConversationItem>,
    mapping: Vec<LineTarget>,
    width: u16,
}

impl Default for Conversation {
    fn default() -> Self {
        let items = sample_items();
        Self {
            component: Textarea::default()
                .borders(
                    Borders::default()
                        .modifiers(BorderType::Rounded)
                        .color(Color::LightBlue),
                )
                .foreground(Color::LightBlue)
                .title("Conversation", Alignment::Left)
                .step(4),
            items,
            mapping: Vec::new(),
            width: 0,
        }
    }
}

fn sample_items() -> Vec<ConversationItem> {
    vec![
        ConversationItem::User(
            "Hello! I'm testing the conversation view. This message should be long enough to wrap and require scrolling.".into(),
        ),
        ConversationItem::Assistant {
            working: WorkingBlock {
                collapsed: false,
                steps: vec![
                    WorkingItem::Thoughts("Analyzing the request".into()),
                    WorkingItem::Tool {
                        name: "search".into(),
                        args: "{\"query\":\"scrolling\"}".into(),
                        result: "{\"answer\":42}".into(),
                        collapsed: true,
                    },
                ],
            },
            response: "Here's an example response after some thinking and a tool call.".into(),
        },
        ConversationItem::User(
            "Can you show more details? Another long line is helpful.".into(),
        ),
        ConversationItem::Assistant {
            working: WorkingBlock {
                collapsed: true,
                steps: vec![
                    WorkingItem::Thoughts("Another thought".into()),
                    WorkingItem::Tool {
                        name: "math".into(),
                        args: "1+1".into(),
                        result: "2".into(),
                        collapsed: true,
                    },
                ],
            },
            response: "Yes, there's more to see.".into(),
        },
        ConversationItem::User(
            "This is a final message to ensure scrolling works properly.".into(),
        ),
        ConversationItem::Assistant {
            working: WorkingBlock {
                collapsed: false,
                steps: vec![WorkingItem::Thoughts("Wrapping things up".into())],
            },
            response: "All done!".into(),
        },
    ]
}

impl Conversation {
    fn update(&mut self) {
        let width = self.width as usize;
        let (lines, mapping) = wrap_conversation_lines(&self.items, width.saturating_sub(2));
        let spans = lines.into_iter().map(TextSpan::from);
        let mut component = std::mem::take(&mut self.component);
        component = component.text_rows(spans);
        self.component = component;
        self.mapping = mapping;
    }
}

fn wrap_conversation_lines(
    items: &[ConversationItem],
    width: usize,
) -> (Vec<String>, Vec<LineTarget>) {
    let mut lines = Vec::new();
    let mut mapping = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        match item {
            ConversationItem::User(text) => {
                let inner_width = width.saturating_sub(7);
                let wrapped = wrap(text, inner_width.max(1));
                let box_width = wrapped.iter().map(|l| l.len()).max().unwrap_or(0);
                lines.push(format!("     ┌{}┐", "─".repeat(box_width)));
                mapping.push(LineTarget::None);
                for w in wrapped {
                    let mut line = w.into_owned();
                    line.push_str(&" ".repeat(box_width.saturating_sub(line.len())));
                    lines.push(format!("     │{}│", line));
                    mapping.push(LineTarget::None);
                }
                lines.push(format!("     └{}┘", "─".repeat(box_width)));
                mapping.push(LineTarget::None);
                lines.push(String::new());
                mapping.push(LineTarget::None);
            }
            ConversationItem::Assistant { working, response } => {
                let arrow = if working.collapsed { "›" } else { "⌄" };
                lines.push(format!("Working {arrow}"));
                mapping.push(LineTarget::Working { item: idx });
                if !working.collapsed {
                    for (s_idx, step) in working.steps.iter().enumerate() {
                        match step {
                            WorkingItem::Thoughts(t) => {
                                lines.push(format!("· {t}"));
                                mapping.push(LineTarget::None);
                            }
                            WorkingItem::Tool {
                                name,
                                args,
                                result,
                                collapsed,
                            } => {
                                let arrow = if *collapsed { "›" } else { "⌄" };
                                lines.push(format!("· _{name}_ {arrow}"));
                                mapping.push(LineTarget::Tool {
                                    item: idx,
                                    step: s_idx,
                                });
                                if !*collapsed {
                                    lines.push(format!("  args: {args}"));
                                    mapping.push(LineTarget::None);
                                    lines.push(format!("  result: {result}"));
                                    mapping.push(LineTarget::None);
                                }
                            }
                        }
                    }
                }
                lines.push(response.clone());
                mapping.push(LineTarget::None);
                lines.push(String::new());
                mapping.push(LineTarget::None);
            }
        }
    }
    (lines, mapping)
}

impl MockComponent for Conversation {
    fn view(&mut self, frame: &mut Frame, area: tuirealm::ratatui::layout::Rect) {
        if area.width != self.width {
            self.width = area.width;
            self.update();
        }
        self.component.view(frame, area);
    }

    fn query(&self, attr: Attribute) -> Option<AttrValue> {
        self.component.query(attr)
    }

    fn attr(&mut self, attr: Attribute, value: AttrValue) {
        self.component.attr(attr, value);
    }

    fn state(&self) -> State {
        self.component.state()
    }

    fn perform(&mut self, cmd: Cmd) -> CmdResult {
        self.component.perform(cmd)
    }
}

impl Component<Msg, NoUserEvent> for Conversation {
    fn on(&mut self, ev: Event<NoUserEvent>) -> Option<Msg> {
        let _ = match ev {
            Event::Keyboard(KeyEvent {
                code: Key::Down, ..
            }) => self.perform(Cmd::Move(Direction::Down)),
            Event::Keyboard(KeyEvent { code: Key::Up, .. }) => {
                self.perform(Cmd::Move(Direction::Up))
            }
            Event::Keyboard(KeyEvent {
                code: Key::PageDown,
                ..
            }) => self.perform(Cmd::Scroll(Direction::Down)),
            Event::Keyboard(KeyEvent {
                code: Key::PageUp, ..
            }) => self.perform(Cmd::Scroll(Direction::Up)),
            Event::Keyboard(KeyEvent {
                code: Key::Home, ..
            }) => self.perform(Cmd::GoTo(Position::Begin)),
            Event::Keyboard(KeyEvent { code: Key::End, .. }) => {
                self.perform(Cmd::GoTo(Position::End))
            }
            Event::Keyboard(KeyEvent {
                code: Key::Enter, ..
            }) => {
                let idx = self.component.states.list_index;
                match self.mapping.get(idx) {
                    Some(LineTarget::Working { item }) => {
                        if let ConversationItem::Assistant { working, .. } = &mut self.items[*item]
                        {
                            working.collapsed = !working.collapsed;
                            self.update();
                        }
                    }
                    Some(LineTarget::Tool { item, step }) => {
                        if let ConversationItem::Assistant { working, .. } = &mut self.items[*item]
                        {
                            if let WorkingItem::Tool { collapsed, .. } = &mut working.steps[*step] {
                                *collapsed = !*collapsed;
                                self.update();
                            }
                        }
                    }
                    _ => {}
                }
                CmdResult::None
            }
            Event::Keyboard(KeyEvent { code: Key::Tab, .. }) => return Some(Msg::FocusInput),
            Event::Keyboard(KeyEvent { code: Key::Esc, .. }) => return Some(Msg::AppClose),
            _ => CmdResult::None,
        };
        Some(Msg::None)
    }
}
