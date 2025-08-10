use textwrap::wrap;
use tuirealm::event::{Key, KeyEvent, MouseButton, MouseEventKind};
use tuirealm::ratatui::Frame;
use tuirealm::ratatui::layout::Rect;
use tuirealm::ratatui::style::{Color, Style};
use tuirealm::ratatui::text::{Line, Span};
use tuirealm::ratatui::widgets::{Block, Borders, Paragraph};
use tuirealm::{Component, Event, MockComponent, NoUserEvent};
use unicode_width::UnicodeWidthStr;

use crate::Msg;

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
    fn on_key(&mut self, _key: Key) -> bool {
        false
    }
    fn click(&mut self, _line: u16) {
        self.activate();
    }
}

pub struct UserBubble {
    text: String,
    cache_width: u16,
    cache_rev: u64,
    content_rev: u64,
    lines: Vec<String>,
}

impl UserBubble {
    fn new(text: String) -> Self {
        Self {
            text,
            cache_width: 0,
            cache_rev: 0,
            content_rev: 0,
            lines: Vec::new(),
        }
    }

    fn ensure_cache(&mut self, width: u16) {
        if self.cache_width == width && self.cache_rev == self.content_rev {
            return;
        }
        self.cache_width = width;
        self.cache_rev = self.content_rev;
        let inner = width.saturating_sub(7) as usize;
        let wrapped = wrap(&self.text, inner.max(1));
        let box_width = wrapped
            .iter()
            .map(|l| UnicodeWidthStr::width(l.as_ref()))
            .max()
            .unwrap_or(0);
        let mut lines = Vec::new();
        lines.push(format!("     ┌{}┐", "─".repeat(box_width)));
        for w in wrapped {
            let mut line = w.into_owned();
            let width = UnicodeWidthStr::width(line.as_str());
            line.push_str(&" ".repeat(box_width.saturating_sub(width)));
            lines.push(format!("     │{}│", line));
        }
        lines.push(format!("     └{}┘", "─".repeat(box_width)));
        lines.push(String::new());
        self.lines = lines;
    }
}

impl ConvNode for UserBubble {
    fn height(&mut self, width: u16) -> u16 {
        self.ensure_cache(width);
        self.lines.len() as u16
    }

    fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        selected: bool,
        start: u16,
        max_height: u16,
    ) {
        self.ensure_cache(area.width);
        let style = if selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let start = start as usize;
        let end = (start + max_height as usize).min(self.lines.len());
        let lines: Vec<Line> = self.lines[start..end]
            .iter()
            .map(|l| Line::from(Span::styled(l.clone(), style)))
            .collect();
        let para = Paragraph::new(lines);
        frame.render_widget(para, area);
    }
}

pub struct ThoughtStep {
    text: String,
    cache_width: u16,
    cache_rev: u64,
    content_rev: u64,
    lines: Vec<String>,
}

impl ThoughtStep {
    fn new(text: String) -> Self {
        Self {
            text,
            cache_width: 0,
            cache_rev: 0,
            content_rev: 0,
            lines: Vec::new(),
        }
    }

    fn ensure_cache(&mut self, width: u16) {
        if self.cache_width == width && self.cache_rev == self.content_rev {
            return;
        }
        self.cache_width = width;
        self.cache_rev = self.content_rev;
        let inner = width.saturating_sub(2) as usize;
        let wrapped = wrap(&self.text, inner.max(1));
        let mut lines = Vec::new();
        for (i, w) in wrapped.into_iter().enumerate() {
            if i == 0 {
                lines.push(format!("· {}", w));
            } else {
                lines.push(format!("  {}", w));
            }
        }
        self.lines = lines;
    }
}

impl ConvNode for ThoughtStep {
    fn height(&mut self, width: u16) -> u16 {
        self.ensure_cache(width);
        self.lines.len() as u16
    }

    fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        selected: bool,
        start: u16,
        max_height: u16,
    ) {
        self.ensure_cache(area.width);
        let style = if selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let start = start as usize;
        let end = (start + max_height as usize).min(self.lines.len());
        let lines: Vec<Line> = self.lines[start..end]
            .iter()
            .map(|l| Line::from(Span::styled(l.clone(), style)))
            .collect();
        let para = Paragraph::new(lines);
        frame.render_widget(para, area);
    }
}

pub struct ToolStep {
    name: String,
    args: String,
    result: String,
    collapsed: bool,
    cache_width: u16,
    cache_rev: u64,
    content_rev: u64,
    lines: Vec<String>,
}

impl ToolStep {
    fn new(name: String, args: String, result: String, collapsed: bool) -> Self {
        Self {
            name,
            args,
            result,
            collapsed,
            cache_width: 0,
            cache_rev: 0,
            content_rev: 0,
            lines: Vec::new(),
        }
    }

    fn ensure_cache(&mut self, width: u16) {
        if self.cache_width == width && self.cache_rev == self.content_rev {
            return;
        }
        self.cache_width = width;
        self.cache_rev = self.content_rev;
        let mut lines = Vec::new();
        let arrow = if self.collapsed { "›" } else { "⌄" };
        lines.push(format!("· _{}_ {}", self.name, arrow));
        if !self.collapsed {
            let a_wrap = wrap(&self.args, width.saturating_sub(8) as usize);
            for (i, w) in a_wrap.into_iter().enumerate() {
                if i == 0 {
                    lines.push(format!("  args: {}", w));
                } else {
                    lines.push(format!("        {}", w));
                }
            }
            let r_wrap = wrap(&self.result, width.saturating_sub(10) as usize);
            for (i, w) in r_wrap.into_iter().enumerate() {
                if i == 0 {
                    lines.push(format!("  result: {}", w));
                } else {
                    lines.push(format!("          {}", w));
                }
            }
        }
        self.lines = lines;
    }
}

impl ConvNode for ToolStep {
    fn height(&mut self, width: u16) -> u16 {
        self.ensure_cache(width);
        self.lines.len() as u16
    }

    fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        selected: bool,
        start: u16,
        max_height: u16,
    ) {
        self.ensure_cache(area.width);
        let style = if selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let start = start as usize;
        let end = (start + max_height as usize).min(self.lines.len());
        let lines: Vec<Line> = self.lines[start..end]
            .iter()
            .map(|l| Line::from(Span::styled(l.clone(), style)))
            .collect();
        let para = Paragraph::new(lines);
        frame.render_widget(para, area);
    }

    fn activate(&mut self) {
        self.collapsed = !self.collapsed;
        self.content_rev += 1;
    }
}

pub enum Node {
    User(UserBubble),
    Assistant(AssistantBlock),
    Thought(ThoughtStep),
    Tool(ToolStep),
}

pub struct AssistantBlock {
    working_collapsed: bool,
    steps: Vec<Node>,
    response: String,
    cache_width: u16,
    cache_rev: u64,
    content_rev: u64,
    response_lines: Vec<String>,
    selected: usize,
}

impl AssistantBlock {
    fn new(working_collapsed: bool, steps: Vec<Node>, response: String) -> Self {
        Self {
            working_collapsed,
            steps,
            response,
            cache_width: 0,
            cache_rev: 0,
            content_rev: 0,
            response_lines: Vec::new(),
            selected: 0,
        }
    }

    fn ensure_cache(&mut self, width: u16) {
        if self.cache_width == width && self.cache_rev == self.content_rev {
            return;
        }
        self.cache_width = width;
        self.cache_rev = self.content_rev;
        let wrapped = wrap(&self.response, width as usize);
        self.response_lines = wrapped.into_iter().map(|l| l.into_owned()).collect();
        self.response_lines.push(String::new());
    }

    fn total_items(&self) -> usize {
        let steps = if self.working_collapsed {
            0
        } else {
            self.steps.len()
        };
        // working header + steps + response
        1 + steps + 1
    }
}

impl ConvNode for AssistantBlock {
    fn height(&mut self, width: u16) -> u16 {
        self.ensure_cache(width);
        let mut h = 1; // working header
        if !self.working_collapsed {
            for step in &mut self.steps {
                h += step.height(width);
            }
        }
        h += self.response_lines.len() as u16;
        h
    }

    fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        selected: bool,
        start: u16,
        max_height: u16,
    ) {
        self.ensure_cache(area.width);
        let arrow = if self.working_collapsed { "›" } else { "⌄" };
        let sel_style = |is_sel| {
            if selected && is_sel {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            }
        };

        let mut y = area.y;
        let mut remaining = max_height;
        let mut line_idx = 0u16;

        // working header
        if start == 0 && remaining > 0 {
            let header = Paragraph::new(Line::from(Span::styled(
                format!("Working {arrow}"),
                sel_style(self.selected == 0),
            )));
            frame.render_widget(header, Rect { height: 1, ..area });
            y += 1;
            remaining -= 1;
        }
        line_idx += 1;
        if remaining == 0 {
            return;
        }

        // steps
        if !self.working_collapsed {
            for (i, step) in self.steps.iter_mut().enumerate() {
                let h = step.height(area.width);
                if line_idx + h <= start {
                    line_idx += h;
                    continue;
                }
                let offset = if start > line_idx {
                    start - line_idx
                } else {
                    0
                };
                let avail = remaining.min(h - offset);
                let rect = Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: avail,
                };
                step.render(
                    frame,
                    rect,
                    selected && self.selected == i + 1,
                    offset,
                    avail,
                );
                y += avail;
                remaining -= avail;
                line_idx += h;
                if remaining == 0 {
                    return;
                }
            }
        } else {
            line_idx += self
                .steps
                .iter_mut()
                .map(|s| s.height(area.width))
                .sum::<u16>();
        }

        // response
        if remaining > 0 {
            let resp_total = self.response_lines.len() as u16;
            if line_idx + resp_total <= start {
                return;
            }
            let offset = if start > line_idx {
                start - line_idx
            } else {
                0
            };
            let visible = remaining.min(resp_total - offset);
            let style = sel_style(self.selected == self.total_items() - 1);
            let start_idx = offset as usize;
            let end_idx = (start_idx + visible as usize).min(self.response_lines.len());
            let lines: Vec<Line> = self.response_lines[start_idx..end_idx]
                .iter()
                .map(|l| Line::from(Span::styled(l.clone(), style)))
                .collect();
            let rect = Rect {
                x: area.x,
                y,
                width: area.width,
                height: visible,
            };
            let para = Paragraph::new(lines);
            frame.render_widget(para, rect);
        }
    }

    fn activate(&mut self) {
        if self.selected == 0 {
            self.working_collapsed = !self.working_collapsed;
        } else {
            let idx = self.selected - 1;
            if idx < self.steps.len() {
                self.steps[idx].activate();
            }
        }
        self.content_rev += 1;
    }

    fn on_key(&mut self, key: Key) -> bool {
        match key {
            Key::Down => {
                let max = self.total_items() - 1;
                if self.selected < max {
                    self.selected += 1;
                    true
                } else {
                    false
                }
            }
            Key::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn click(&mut self, line: u16) {
        if line == 0 {
            self.selected = 0;
        } else {
            let mut pos = 1;
            if !self.working_collapsed {
                for (i, step) in self.steps.iter_mut().enumerate() {
                    let h = step.height(self.cache_width);
                    if line < pos + h {
                        self.selected = i + 1;
                        self.activate();
                        return;
                    }
                    pos += h;
                }
            }
            self.selected = self.total_items() - 1;
        }
        self.activate();
    }
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

    fn on_key(&mut self, key: Key) -> bool {
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

pub struct Conversation {
    items: Vec<Node>,
    selected: usize,
    scroll: u16,
    layout: Vec<(u16, u16)>,
    width: u16,
    viewport: u16,
    dirty: bool,
    area: Rect,
    focused: bool,
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
    fn ensure_layout(&mut self, width: u16) {
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

    fn total_height(&self) -> u16 {
        self.layout.last().map(|(s, h)| s + h).unwrap_or(0)
    }

    fn ensure_visible(&mut self) {
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
        self.dirty = true;
    }

    pub fn push_assistant(&mut self) {
        self.items.push(Node::Assistant(AssistantBlock::new(
            false,
            Vec::new(),
            String::new(),
        )));
        self.dirty = true;
    }

    pub fn append_thinking(&mut self, text: &str) {
        let block = self.ensure_last_assistant();
        if let Some(Node::Thought(t)) = block.steps.last_mut() {
            t.text.push_str(text);
            t.content_rev += 1;
        } else {
            block
                .steps
                .push(Node::Thought(ThoughtStep::new(text.into())));
        }
        block.content_rev += 1;
        self.dirty = true;
    }

    pub fn append_response(&mut self, text: &str) {
        let block = self.ensure_last_assistant();
        block.response.push_str(text);
        block.content_rev += 1;
        self.dirty = true;
    }

    pub fn add_step(&mut self, mut step: Node) {
        match &mut step {
            Node::Thought(t) => t.content_rev += 1,
            Node::Tool(t) => t.content_rev += 1,
            _ => {}
        }
        let block = self.ensure_last_assistant();
        block.steps.push(step);
        block.content_rev += 1;
        self.dirty = true;
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
