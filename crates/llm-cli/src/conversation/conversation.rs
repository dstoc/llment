use crossterm::event::{Event, MouseButton, MouseEventKind};
use ratatui::{Frame, layout::Rect};

use crate::component::Component;

use super::node::ConvNode;
use super::{Node, ThoughtStep, UserBubble, assistant_block::AssistantBlock, tool_step::ToolStep};

pub struct Conversation {
    items: Vec<Node>,
    scroll: u16,
    layout: Vec<(u16, u16)>,
    width: u16,
    viewport: u16,
    needs_layout: bool,
    area: Rect,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            scroll: 0,
            layout: Vec::new(),
            width: 0,
            viewport: 0,
            needs_layout: true,
            area: Rect::default(),
        }
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}

impl Conversation {
    pub(crate) fn ensure_layout(&mut self, width: u16) {
        if self.width != width || self.needs_layout {
            self.width = width;
            self.layout.clear();
            let mut pos = 0;
            for item in self.items.iter_mut() {
                let h = item.height(width);
                self.layout.push((pos, h));
                pos += h;
            }
            self.needs_layout = false;
            let max = self.total_height().saturating_sub(self.viewport);
            if self.scroll > max {
                self.scroll = max;
            }
        }
    }

    pub(crate) fn total_height(&self) -> u16 {
        self.layout.last().map(|(s, h)| s + h).unwrap_or(0)
    }

    pub(crate) fn is_at_bottom(&self) -> bool {
        let max = self.total_height().saturating_sub(self.viewport);
        self.scroll >= max
    }

    pub(crate) fn scroll_to_bottom(&mut self) {
        let max = self.total_height().saturating_sub(self.viewport);
        self.scroll = max;
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.scroll = 0;
        self.layout.clear();
        self.needs_layout = true;
    }
}

impl Component for Conversation {
    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Mouse(mouse_event) => match mouse_event.kind {
                MouseEventKind::ScrollUp => {
                    self.scroll = self.scroll.saturating_sub(1);
                    let max = self.total_height().saturating_sub(self.viewport);
                    if self.scroll > max {
                        self.scroll = max;
                    }
                }
                MouseEventKind::ScrollDown => {
                    let max = self.total_height().saturating_sub(self.viewport);
                    self.scroll = (self.scroll + 1).min(max);
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    let snap = self.viewport.saturating_sub(self.total_height());
                    if mouse_event.row >= self.area.y + snap {
                        let line = self.scroll + (mouse_event.row - self.area.y - snap) as u16;
                        let mut target: Option<(usize, u16)> = None;
                        for (i, (start, h)) in self.layout.iter().enumerate() {
                            if line >= *start && line < *start + *h {
                                target = Some((i, *start));
                                break;
                            }
                        }
                        if let Some((idx, start)) = target {
                            let rel = line - start;
                            self.items[idx].click(rel);
                            self.needs_layout = true;
                            self.ensure_layout(self.width);
                            let max = self.total_height().saturating_sub(self.viewport);
                            if self.scroll > max {
                                self.scroll = max;
                            }
                        }
                    }
                }
                _ => (),
            },
            _ => (),
        }
    }
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        self.viewport = area.height;
        self.area = area;
        self.ensure_layout(area.width);
        let total = self.total_height();
        let snap = self.viewport.saturating_sub(total);

        for (idx, item) in self.items.iter_mut().enumerate() {
            let (start, h) = self.layout[idx];
            if start + h <= self.scroll {
                continue;
            }
            if start >= self.scroll + self.viewport {
                break;
            }
            let offset = self.scroll.saturating_sub(start);
            let y = area.y + snap + start.saturating_sub(self.scroll);
            let remaining = (area.y + self.viewport).saturating_sub(y);
            let max_height = remaining.min(h - offset);
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: max_height,
            };
            item.render(frame, rect, false, offset, max_height);
        }
    }
}

impl Conversation {
    fn ensure_last_assistant(&mut self) -> &mut AssistantBlock {
        if !matches!(self.items.last(), Some(Node::Assistant(_))) {
            self.items.push(Node::Assistant(AssistantBlock::new(
                false,
                Vec::new(),
                String::new(),
            )));
        }
        match self.items.last_mut().unwrap() {
            Node::Assistant(block) => block,
            _ => unreachable!(),
        }
    }

    pub fn push_user(&mut self, text: String) {
        self.items.push(Node::User(UserBubble::new(text)));
        self.needs_layout = true;
        self.ensure_layout(self.width);
        self.scroll_to_bottom();
    }

    pub fn push_assistant_block(&mut self) {
        let at_bottom = self.is_at_bottom();
        self.items.push(Node::Assistant(AssistantBlock::new(
            false,
            Vec::new(),
            String::new(),
        )));
        self.needs_layout = true;
        self.ensure_layout(self.width);
        if at_bottom {
            self.scroll_to_bottom();
        }
    }

    pub fn append_thinking(&mut self, text: &str) {
        let at_bottom = self.is_at_bottom();
        let block = self.ensure_last_assistant();
        block.record_activity();
        if let Some(Node::Thought(t)) = block.steps.last_mut() {
            t.text.push_str(text);
            t.content_rev += 1;
        } else {
            block
                .steps
                .push(Node::Thought(ThoughtStep::new(text.into())));
        }
        block.content_rev += 1;
        self.needs_layout = true;
        self.ensure_layout(self.width);
        if at_bottom {
            self.scroll_to_bottom();
        }
    }

    pub fn append_response(&mut self, text: &str) {
        let at_bottom = self.is_at_bottom();
        let block = self.ensure_last_assistant();
        block.record_activity();
        block.response.push_str(text);
        block.content_rev += 1;
        self.needs_layout = true;
        self.ensure_layout(self.width);
        if at_bottom {
            self.scroll_to_bottom();
        }
    }

    pub fn add_tool_step(&mut self, step: ToolStep) {
        let step = Node::Tool(step);
        let at_bottom = self.is_at_bottom();
        let block = self.ensure_last_assistant();
        block.record_activity();
        block.steps.push(step);
        block.content_rev += 1;
        self.needs_layout = true;
        self.ensure_layout(self.width);
        if at_bottom {
            self.scroll_to_bottom();
        }
    }

    pub fn update_tool_result(&mut self, step_id: usize, result: String, failed: bool) {
        let at_bottom = self.is_at_bottom();
        let block = self.ensure_last_assistant();
        let matching = block.steps.iter().enumerate().find(|(_i, step)| {
            if let Node::Tool(tool) = step {
                if tool.id == step_id {
                    return true;
                }
            }
            false
        });
        if let Some((step_idx, _)) = matching {
            if let Some(Node::Tool(ToolStep {
                result: r,
                done,
                failed: f,
                content_rev,
                ..
            })) = block.steps.get_mut(step_idx)
            {
                *r = result;
                *done = true;
                *f = failed;
                *content_rev += 1;
                block.record_activity();
                block.content_rev += 1;
                self.needs_layout = true;
                self.ensure_layout(self.width);
                if at_bottom {
                    self.scroll_to_bottom();
                }
            }
        }
    }

    pub fn redo_last(&mut self) -> Option<String> {
        if matches!(self.items.last(), Some(Node::Assistant(_))) {
            self.items.pop();
            if let Some(Node::User(user)) = self.items.pop() {
                self.needs_layout = true;
                self.ensure_layout(self.width);
                self.scroll_to_bottom();
                return Some(user.text);
            }
        }
        None
    }
}
