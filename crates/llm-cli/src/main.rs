use std::io::stdout;

use crossterm::{
    event::Event as CrosEvent,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tuirealm::{
    Component, MockComponent,
    event::{Event, NoUserEvent},
    ratatui::backend::CrosstermBackend,
};

use llm_core::{ResponseChunk, ResponseMessage};

mod components;
use components::{App, AppMsg};

enum EventMessage {
    Ui(Event<NoUserEvent>),
    Llm(ResponseChunk),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::unbounded_channel::<EventMessage>();

    let tx_term = tx.clone();
    tokio::spawn(async move {
        let mut reader = crossterm::event::EventStream::new();
        while let Some(Ok(ev)) = reader.next().await {
            if let CrosEvent::Key(key) = ev {
                if tx_term
                    .send(EventMessage::Ui(Event::Keyboard(key.into())))
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    let mut app = App::new();

    while let Some(msg) = rx.recv().await {
        match msg {
            EventMessage::Ui(ev) => {
                if let Some(app_msg) = app.on(ev) {
                    if let AppMsg::Send(text) = app_msg {
                        let tx_llm = tx.clone();
                        tokio::spawn(async move {
                            let chunk = ResponseChunk {
                                message: ResponseMessage {
                                    content: format!("Echo: {}", text),
                                    tool_calls: Vec::new(),
                                    thinking: None,
                                },
                                done: true,
                            };
                            let _ = tx_llm.send(EventMessage::Llm(chunk));
                        });
                    }
                }
                terminal.draw(|f| app.view(f, f.size()))?;
            }
            EventMessage::Llm(chunk) => {
                app.push_assistant(chunk.message.content);
                terminal.draw(|f| app.view(f, f.size()))?;
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
