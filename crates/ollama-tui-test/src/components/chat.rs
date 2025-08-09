use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use tuirealm::{
    AttrValue, Attribute, Component, Event, MockComponent, State,
    command::{Cmd, CmdResult},
};

use super::history::HistoryItem;
use crate::markdown;

pub struct Chat {
    pub items: Vec<HistoryItem>,
    pub scroll: i32,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            scroll: 0,
        }
    }
}

fn render_items(items: &[HistoryItem], width: usize) -> Vec<Line> {
    use super::history::HistoryItem::*;
    let mut rendered = Vec::new();
    for item in items {
        let lines = match item {
            User(u) => u.render(width),
            Assistant(a) => a.render(),
            Thinking(t) => t.render(width),
            Separator(s) => s.render(),
            Error(e) => e.render(),
        };
        for (line, markdown, error) in lines {
            if markdown {
                let mut converted = markdown::markdown_to_lines(&line, width);
                if error {
                    if let Some(first) = converted.first_mut() {
                        let styled = first.clone().patch_style(
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::ITALIC),
                        );
                        *first = styled;
                    }
                }
                rendered.extend(converted);
            } else if error {
                rendered.push(Line::styled(
                    line,
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::ITALIC),
                ));
            } else {
                rendered.push(Line::raw(line));
            }
        }
    }
    rendered
}

impl MockComponent for Chat {
    fn view(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(1)].as_ref())
            .split(area);
        let content = chunks[0];
        let width = content.width as usize;
        let lines = render_items(&self.items, width);
        let height = content.height as usize;
        let line_count = lines.len();
        let max_scroll = line_count.saturating_sub(height);
        self.scroll = self.scroll.clamp(0, max_scroll as i32);
        let top_line = line_count.saturating_sub(height + self.scroll as usize);
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((top_line as u16, 0));
        f.render_widget(paragraph, content);
        if line_count > height {
            let mut scrollbar_state = ScrollbarState::new((max_scroll as usize) + 1)
                .position(top_line)
                .viewport_content_length(height);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
            f.render_stateful_widget(scrollbar, chunks[1], &mut scrollbar_state);
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

impl Component<(), ()> for Chat {
    fn on(&mut self, ev: Event<()>) -> Option<()> {
        use tuirealm::event::{Key, MouseEventKind};
        match ev {
            Event::Keyboard(key) => match key.code {
                Key::Up => self.scroll += 1,
                Key::Down => self.scroll -= 1,
                Key::Char('t') => {
                    if let Some(item) = self
                        .items
                        .iter_mut()
                        .rev()
                        .find(|i| matches!(i, super::history::HistoryItem::Thinking(_)))
                    {
                        if let super::history::HistoryItem::Thinking(t) = item {
                            t.collapsed = !t.collapsed;
                        }
                    }
                }
                _ => {}
            },
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollUp => self.scroll += 1,
                MouseEventKind::ScrollDown => self.scroll -= 1,
                _ => {}
            },
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::history;
    use insta::assert_snapshot;
    use ratatui::{
        Terminal,
        backend::TestBackend,
        buffer::Buffer,
        layout::{Constraint, Direction, Layout, Rect},
    };
    use tuirealm::MockComponent;

    fn buffer_to_string(buffer: &Buffer) -> String {
        let area = buffer.area;
        let mut lines = Vec::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    fn draw(chat: &mut Chat, terminal: &mut Terminal<TestBackend>) {
        terminal
            .draw(|f| {
                let area = f.area();
                let content_width = area.width.saturating_sub(1).min(100);
                let total_width = content_width + 1;
                let x_offset = (area.width.saturating_sub(total_width)) / 2;
                let centered = Rect::new(area.x + x_offset, area.y, total_width, area.height);
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(1), Constraint::Length(1)].as_ref())
                    .split(centered);
                chat.view(f, chunks[0]);
            })
            .unwrap();
    }

    #[test]
    fn chat_renders_user_message() {
        let backend = TestBackend::new(20, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut chat = Chat::default();
        chat.items
            .push(HistoryItem::User(history::UserItem("Hello".into())));
        draw(&mut chat, &mut terminal);
        let buffer = terminal.backend().buffer().clone();
        let rendered = buffer_to_string(&buffer);
        let trimmed = rendered
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string();
        assert_snapshot!(trimmed, @r###"     ┌─────┐
     │Hello│
     └─────┘
"###);
    }

    #[test]
    fn chat_renders_assistant_message() {
        let backend = TestBackend::new(20, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut chat = Chat::default();
        chat.items
            .push(HistoryItem::Assistant(history::AssistantItem(
                "Hello".into(),
            )));
        draw(&mut chat, &mut terminal);
        let buffer = terminal.backend().buffer().clone();
        let rendered = buffer_to_string(&buffer);
        let trimmed = rendered
            .lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string();
        assert_snapshot!(trimmed, @r###"Hello
"###);
    }
}
