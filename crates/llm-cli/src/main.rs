use std::io::stdout;
use std::time::Duration;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

use tuirealm::EventListenerCfg;
use tuirealm::application::PollStrategy;
use tuirealm::ratatui::layout::{Constraint, Direction as LayoutDirection, Layout};
use tuirealm::terminal::{CrosstermTerminalAdapter, TerminalBridge};
use tuirealm::{Application, NoUserEvent, Sub, SubClause, SubEventClause, Update};

mod components;
use components::{Conversation, Prompt};

#[derive(Debug, PartialEq)]
pub enum Msg {
    AppClose,
    FocusConversation,
    FocusInput,
    None,
}

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub enum Id {
    Conversation,
    Input,
}

struct Model {
    app: Application<Id, Msg, NoUserEvent>,
    quit: bool,
    redraw: bool,
}

impl Default for Model {
    fn default() -> Self {
        let mut app: Application<Id, Msg, NoUserEvent> = Application::init(
            EventListenerCfg::default().crossterm_input_listener(Duration::from_millis(10), 10),
        );
        assert!(
            app.mount(
                Id::Conversation,
                Box::new(Conversation::default()),
                vec![Sub::new(SubEventClause::Any, SubClause::Always)],
            )
            .is_ok()
        );
        assert!(
            app.mount(Id::Input, Box::new(Prompt::default()), vec![])
                .is_ok()
        );
        assert!(app.active(&Id::Input).is_ok());
        Self {
            app,
            quit: false,
            redraw: true,
        }
    }
}

impl Model {
    fn view(&mut self, terminal: &mut TerminalBridge<CrosstermTerminalAdapter>) {
        let _ = terminal.raw_mut().draw(|f| {
            let chunks = Layout::default()
                .direction(LayoutDirection::Vertical)
                .margin(1)
                .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
                .split(f.area());
            self.app.view(&Id::Conversation, f, chunks[0]);
            self.app.view(&Id::Input, f, chunks[1]);
        });
    }
}

impl Update<Msg> for Model {
    fn update(&mut self, msg: Option<Msg>) -> Option<Msg> {
        self.redraw = true;
        match msg.unwrap_or(Msg::None) {
            Msg::AppClose => {
                self.quit = true;
                None
            }
            Msg::FocusConversation => {
                let _ = self.app.active(&Id::Conversation);
                None
            }
            Msg::FocusInput => {
                let _ = self.app.active(&Id::Input);
                None
            }
            Msg::None => None,
        }
    }
}

fn main() {
    let mut model = Model::default();
    let mut terminal = TerminalBridge::init_crossterm().expect("Cannot create terminal bridge");
    let _ = terminal.enable_raw_mode();
    let _ = terminal.enter_alternate_screen();
    let _ = execute!(stdout(), EnableMouseCapture);

    while !model.quit {
        if let Ok(messages) = model.app.tick(PollStrategy::Once) {
            for msg in messages {
                let mut current = Some(msg);
                while let Some(m) = current {
                    current = model.update(Some(m));
                }
            }
        }
        if model.redraw {
            model.view(&mut terminal);
            model.redraw = false;
        }
    }

    let _ = execute!(stdout(), DisableMouseCapture);
    let _ = terminal.leave_alternate_screen();
    let _ = terminal.disable_raw_mode();
    let _ = terminal.clear_screen();
}
