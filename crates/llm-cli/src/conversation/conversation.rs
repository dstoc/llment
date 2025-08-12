use tuirealm::event::{MouseButton, MouseEventKind};
use tuirealm::ratatui::{Frame, layout::Rect};
use tuirealm::{Component, Event, MockComponent, NoUserEvent};

use crate::Msg;

use super::{Node, node::ConvNode};

pub struct Conversation {
    pub(crate) items: Vec<Node>,
    pub(crate) scroll: u16,
    pub(crate) layout: Vec<(u16, u16)>,
    pub(crate) width: u16,
    pub(crate) viewport: u16,
    pub(crate) dirty: bool,
    pub(crate) area: Rect,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            scroll: 0,
            layout: Vec::new(),
            width: 0,
            viewport: 0,
            dirty: true,
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
        self.dirty = true;
    }
}

impl MockComponent for Conversation {
    fn view(&mut self, frame: &mut Frame, area: Rect) {
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

    fn query(&self, _attr: tuirealm::Attribute) -> Option<tuirealm::AttrValue> {
        None
    }

    fn attr(&mut self, _attr: tuirealm::Attribute, _value: tuirealm::AttrValue) {}

    fn state(&self) -> tuirealm::State {
        tuirealm::State::None
    }

    fn perform(&mut self, _cmd: tuirealm::command::Cmd) -> tuirealm::command::CmdResult {
        tuirealm::command::CmdResult::None
    }
}

impl Component<Msg, NoUserEvent> for Conversation {
    fn on(&mut self, ev: Event<NoUserEvent>) -> Option<Msg> {
        if let Event::Mouse(me) = ev {
            if me.column >= self.area.x
                && me.column < self.area.x + self.area.width
                && me.row >= self.area.y
                && me.row < self.area.y + self.area.height
            {
                match me.kind {
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
                        if me.row >= self.area.y + snap {
                            let line = self.scroll + (me.row - self.area.y - snap) as u16;
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
                                self.dirty = true;
                                self.ensure_layout(self.width);
                                let max = self.total_height().saturating_sub(self.viewport);
                                if self.scroll > max {
                                    self.scroll = max;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Some(Msg::None)
    }
}
