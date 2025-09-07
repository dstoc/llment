use crossterm::event::{Event, MouseButton, MouseEventKind};
use llm::{AssistantPart, ChatMessage, JsonResult};
use ratatui::{Frame, layout::Rect};
use serde_json::to_string;

use crate::component::Component;

use super::node::ConvNode;
use super::{
    Node, ThoughtStep, UserBubble, assistant_block::AssistantBlock, response_step::ResponseStep,
    tool_step::ToolStep,
};

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

    fn adjust_layout_after_change(&mut self, idx: usize, start: u16, prev_height: u16) {
        self.needs_layout = true;
        self.ensure_layout(self.width);
        let new_height = self.layout[idx].1;
        if start < self.scroll {
            if new_height < prev_height {
                self.scroll = self.scroll.saturating_sub(prev_height - new_height);
            } else {
                self.scroll = self.scroll.saturating_add(new_height - prev_height);
            }
        }
        let max = self.total_height().saturating_sub(self.viewport);
        if self.scroll > max {
            self.scroll = max;
        }
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
                            let prev_height = self.layout[idx].1;
                            self.items[idx].click(rel);
                            self.adjust_layout_after_change(idx, start, prev_height);
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
            self.needs_layout = true;
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
        if !block.response.is_empty() {
            let resp = std::mem::take(&mut block.response);
            block.steps.push(Node::Response(ResponseStep::new(resp)));
        }
        block.steps.push(step);
        block.content_rev += 1;
        self.needs_layout = true;
        self.ensure_layout(self.width);
        if at_bottom {
            self.scroll_to_bottom();
        }
    }

    pub fn update_tool_result(&mut self, step_id: &str, result: String, failed: bool) -> bool {
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
            true
        } else {
            false
        }
    }

    pub fn redo_last(&mut self) -> Option<String> {
        while !self.items.is_empty() {
            if let Some(Node::User(user)) = self.items.pop() {
                self.needs_layout = true;
                self.ensure_layout(self.width);
                self.scroll_to_bottom();
                return Some(user.text);
            }
        }
        None
    }

    pub fn set_history(&mut self, history: &[ChatMessage]) {
        self.clear();
        for msg in history {
            match msg {
                ChatMessage::User(u) => {
                    self.push_user(u.content.clone());
                }
                ChatMessage::Assistant(a) => {
                    for part in &a.content {
                        match part {
                            AssistantPart::Thinking { text } => {
                                if !text.is_empty() {
                                    self.append_thinking(text);
                                }
                            }
                            AssistantPart::Text { text } => {
                                if !text.is_empty() {
                                    self.append_response(text);
                                }
                            }
                            AssistantPart::ToolCall(call) => {
                                let args = call.arguments_invalid.clone().unwrap_or_else(|| {
                                    to_string(&call.arguments).unwrap_or_default()
                                });
                                self.add_tool_step(ToolStep::new(
                                    call.name.clone(),
                                    call.id.clone(),
                                    args,
                                    String::new(),
                                    false,
                                ));
                            }
                        }
                    }
                }
                ChatMessage::Tool(tmsg) => {
                    let result = match &tmsg.content {
                        JsonResult::Content { content } => to_string(content).unwrap_or_default(),
                        JsonResult::Error { error } => error.clone(),
                    };
                    if !self.update_tool_result(&tmsg.id, result.clone(), false) {
                        let mut step = ToolStep::new(
                            tmsg.tool_name.clone(),
                            tmsg.id.clone(),
                            String::new(),
                            result,
                            false,
                        );
                        step.done = true;
                        self.add_tool_step(step);
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};

    fn render_conv(conv: &mut Conversation, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                conv.render(f, Rect::new(0, 0, width, height));
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_to_debug_string(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in buf.area.top()..buf.area.bottom() {
            let mut prev_fg = None;
            let mut prev_bg = None;
            for x in buf.area.left()..buf.area.right() {
                let c = buf.cell((x, y)).unwrap();
                let fg = c.style().fg;
                let bg = c.style().bg;
                if prev_fg != fg || prev_bg != bg {
                    let fg_str = fg.map(|c| format!("{:?}", c)).unwrap_or_else(|| "_".into());
                    let bg_str = bg.map(|c| format!("{:?}", c)).unwrap_or_else(|| "_".into());
                    out.push_str(&format!("[{},{}]", fg_str, bg_str));
                    prev_fg = fg;
                    prev_bg = bg;
                }
                out.push_str(c.symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn collapsing_block_adjusts_scroll() {
        let mut conv = Conversation::new();
        conv.items.push(Node::Assistant(AssistantBlock::new(
            false,
            vec![Node::Thought(ThoughtStep::new(
                "word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11".into(),
            ))],
            String::new(),
        )));
        conv.items.push(Node::Assistant(AssistantBlock::new(
            false,
            vec![Node::Thought(ThoughtStep::new(
                "word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11".into(),
            ))],
            String::new(),
        )));
        conv.items.push(Node::Assistant(AssistantBlock::new(
            false,
            Vec::new(),
            "hi".into(),
        )));
        conv.needs_layout = true;
        conv.viewport = 5;
        conv.ensure_layout(20);
        conv.scroll = 12;

        let _ = render_conv(&mut conv, 20, 5);

        let prev_scroll = conv.scroll;
        let (start, prev_height) = conv.layout[0];
        conv.items[0].click(0);
        conv.adjust_layout_after_change(0, start, prev_height);
        let new_height = conv.layout[0].1;
        let new_max = conv.total_height().saturating_sub(conv.viewport);
        let mut expected = prev_scroll.min(new_max);
        if start < expected {
            expected = expected.saturating_sub(prev_height - new_height);
        }
        assert_eq!(conv.scroll, expected);

        let buffer = render_conv(&mut conv, 20, 5);
        let dbg = buffer_to_debug_string(&buffer)
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        assert_snapshot!(dbg, @r"[Reset,Reset]Thinking ⠋ ⌄
[Reset,Reset]· word1 word2 word3
[Reset,Reset]│ word4 word5 word6
[Reset,Reset]│ word7 word8 word9
[Reset,Reset]│ word10 word11");
    }
}
