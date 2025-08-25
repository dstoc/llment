use std::error::Error;
use std::io::stdout;
use std::time::Duration;

use app::{App, AppModel};
use component::Component;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyCode, KeyEventKind,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};

use clap::Parser;
use futures::FutureExt;
use ratatui::Terminal;
use ratatui::prelude::CrosstermBackend;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::WatchStream;

mod app;
mod builtins;
mod component;
mod components;
mod conversation;
mod markdown;

use llm::mcp::{McpContext, load_mcp_servers};
use llm::{self, Provider};

#[derive(Parser, Debug)]
pub struct Args {
    #[arg(long, value_enum, default_value_t = Provider::Ollama)]
    provider: Provider,
    /// Model identifier to use
    #[arg(long, default_value = "gpt-oss:20b")]
    model: String,
    /// LLM host URL, e.g. http://localhost:11434 for Ollama
    #[arg(long, default_value = "http://127.0.0.1:11434")]
    host: String,
    /// Path to MCP configuration JSON
    #[arg(long)]
    mcp: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let (mcp_ctx, services) = if let Some(path) = &args.mcp {
        load_mcp_servers(path).await.expect("mcp")
    } else {
        (McpContext::default(), Vec::new())
    };

    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(stdout(), EnterAlternateScreen, crossterm::cursor::Hide)?;
    crossterm::execute!(stdout(), EnableMouseCapture)?;
    crossterm::execute!(stdout(), EnableBracketedPaste)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::unbounded_channel();

    let (needs_redraw_tx, needs_redraw_rx) = watch::channel(false);
    let (needs_update_tx, needs_update_rx) = watch::channel(false);
    let (should_quit_tx, should_quit_rx) = watch::channel(false);
    let mut app = App::new(
        AppModel {
            needs_redraw: needs_redraw_tx.clone(),
            needs_update: needs_update_tx.clone(),
            should_quit: should_quit_tx.clone(),
        },
        args,
    );
    app.init(mcp_ctx, services).await;
    Component::init(&mut app);

    tokio::spawn(event_loop(tx));
    let mut ticker = tokio::time::interval(Duration::from_millis(16));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut redraw_stream = WatchStream::new(needs_redraw_rx.clone());
    let mut update_stream = WatchStream::new(needs_update_rx.clone());
    let mut quit_stream = WatchStream::new(should_quit_rx.clone());
    let mut should_redraw = true;
    let mut should_update = true;

    loop {
        tokio::select! {
            quit = quit_stream.next() => {
                if let Some(true) = quit {
                    break
                }
            }
            redraw = redraw_stream.next() => {
                if let Some(true) = redraw {
                    should_redraw = true;
                }
            }
            update = update_stream.next() => {
                if let Some(true) = update{
                    should_update = true;
                }
            }
            maybe = rx.recv() => {
                let Some(ev) = maybe else { break; };
                match ev {
                    Event::Key(key) => {
                        if key.code == KeyCode::Esc {
                            break;
                        } else {
                            app.handle_event(Event::Key(key));
                        }
                    }
                    other => app.handle_event(other),
                }
            }
            _ = ticker.tick() => {
                if should_update {
                    while *needs_update_rx.borrow() {
                        let _ = needs_update_tx.send(false);
                        app.update();
                    }
                    should_update = false;
                }
                if should_redraw {
                    let _ = needs_redraw_tx.send(false);
                    terminal.draw(|frame| {
                        app.render(frame, frame.area());
                    })?;
                    should_redraw = false;
                }
            }
        }
    }

    crossterm::execute!(stdout(), DisableBracketedPaste)?;
    crossterm::execute!(stdout(), DisableMouseCapture)?;
    crossterm::execute!(stdout(), LeaveAlternateScreen, crossterm::cursor::Show)?;
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}

async fn event_loop(event_tx: UnboundedSender<Event>) {
    let mut event_stream = EventStream::new();
    loop {
        let event = tokio::select! {
            crossterm_event = event_stream.next().fuse() => match crossterm_event {
                Some(Ok(event)) => match event {
                    Event::Key(key) if key.kind == KeyEventKind::Press => Event::Key(key),
                    Event::Mouse(mouse) => Event::Mouse(mouse),
                    Event::Resize(x, y) => Event::Resize(x, y),
                    Event::FocusLost => Event::FocusLost,
                    Event::FocusGained => Event::FocusGained,
                    Event::Paste(s) => Event::Paste(s),
                    _ => continue,
                }
                Some(Err(_)) => break, // Event::Error,
                None => break,
            },
        };
        if event_tx.send(event).is_err() {
            break;
        }
    }
}
