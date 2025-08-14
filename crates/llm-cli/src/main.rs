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
use futures_signals::signal::{Mutable, SignalExt};
use ratatui::Terminal;
use ratatui::prelude::CrosstermBackend;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::time::MissedTickBehavior;
use tokio_stream::StreamExt;

mod app;
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
    let (mcp_ctx, _services) = if let Some(path) = &args.mcp {
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

    let needs_redraw = Mutable::new(false);
    let needs_update = Mutable::new(false);
    let should_quit = Mutable::new(false);
    let mut app = App::new(
        AppModel {
            needs_redraw: needs_redraw.clone(),
            needs_update: needs_update.clone(),
            should_quit: should_quit.clone(),
        },
        args,
        mcp_ctx,
    );
    app.init();

    tokio::spawn(event_loop(tx));
    let mut ticker = tokio::time::interval(Duration::from_millis(16));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut redraw_stream = needs_redraw.signal().to_stream();
    let mut update_stream = needs_update.signal().to_stream();
    let mut quit_stream = should_quit.signal().to_stream();
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
                    // TODO: Do we need a loop?
                    while needs_update.get() {
                        needs_update.set(false);
                        app.update();
                    }
                    should_update = false;
                }
                if should_redraw {
                    needs_redraw.set(false);
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
