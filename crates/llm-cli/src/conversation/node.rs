use crossterm::event::KeyCode;
use ratatui::{Frame, layout::Rect};

use super::{
    assistant_block::AssistantBlock, thought_step::ThoughtStep, tool_step::ToolStep,
    user_bubble::UserBubble,
};

pub trait ConvNode {
    fn height(&mut self, width: u16) -> u16;
    fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        selected: bool,
        start: u16,
        max_height: u16,
    );
    fn activate(&mut self) {}
    fn on_key(&mut self, _key: KeyCode) -> bool {
        false
    }
    fn click(&mut self, _line: u16) {
        self.activate();
    }
}

pub enum Node {
    User(UserBubble),
    Assistant(AssistantBlock),
    Thought(ThoughtStep),
    Tool(ToolStep),
}

impl ConvNode for Node {
    fn height(&mut self, width: u16) -> u16 {
        match self {
            Node::User(n) => n.height(width),
            Node::Assistant(n) => n.height(width),
            Node::Thought(n) => n.height(width),
            Node::Tool(n) => n.height(width),
        }
    }

    fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        selected: bool,
        start: u16,
        max_height: u16,
    ) {
        match self {
            Node::User(n) => n.render(frame, area, selected, start, max_height),
            Node::Assistant(n) => n.render(frame, area, selected, start, max_height),
            Node::Thought(n) => n.render(frame, area, selected, start, max_height),
            Node::Tool(n) => n.render(frame, area, selected, start, max_height),
        }
    }

    fn activate(&mut self) {
        match self {
            Node::User(n) => n.activate(),
            Node::Assistant(n) => n.activate(),
            Node::Thought(n) => n.activate(),
            Node::Tool(n) => n.activate(),
        }
    }

    fn on_key(&mut self, key: KeyCode) -> bool {
        match self {
            Node::User(n) => n.on_key(key),
            Node::Assistant(n) => n.on_key(key),
            Node::Thought(n) => n.on_key(key),
            Node::Tool(n) => n.on_key(key),
        }
    }

    fn click(&mut self, line: u16) {
        match self {
            Node::User(n) => n.click(line),
            Node::Assistant(n) => n.click(line),
            Node::Thought(n) => n.click(line),
            Node::Tool(n) => n.click(line),
        }
    }
}
