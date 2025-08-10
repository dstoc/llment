use std::io::stdout;
use std::sync::Arc;

use crossterm::{
    event::Event as CrosEvent,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::StreamExt;
use tuirealm::{Component, MockComponent, event::Event, ratatui::backend::CrosstermBackend};

use llm_core::ollama::OllamaClient;
use llm_core::{ChatMessage, ChatMessageRequest, LlmClient};

mod components;
mod event;

use components::{App, AppMsg};
use event::ChatEvent;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = mpsc::unbounded_channel::<Event<ChatEvent>>();

    let tx_term = tx.clone();
    tokio::spawn(async move {
        let mut reader = crossterm::event::EventStream::new();
        while let Some(Ok(ev)) = reader.next().await {
            if let CrosEvent::Key(key) = ev {
                if tx_term.send(Event::Keyboard(key.into())).is_err() {
                    break;
                }
            }
        }
    });

    let client: Arc<dyn LlmClient> = Arc::new(OllamaClient::new("http://localhost:11434")?);
    let messages = Arc::new(Mutex::new(Vec::<ChatMessage>::new()));
    let mut app = App::new();

    while let Some(ev) = rx.recv().await {
        let mut should_break = false;
        if let Some(app_msg) = app.on(ev) {
            match app_msg {
                AppMsg::Send(text) => {
                    let tx_llm = tx.clone();
                    let client = client.clone();
                    let msgs = messages.clone();
                    tokio::spawn(async move {
                        {
                            let mut locked = msgs.lock().await;
                            locked.push(ChatMessage::user(text.clone()));
                            let request =
                                ChatMessageRequest::new("llama3.1:8b".to_string(), locked.clone());
                            drop(locked);
                            if let Ok(mut stream) = client.send_chat_messages_stream(request).await
                            {
                                let mut assistant = String::new();
                                while let Some(chunk) = stream.next().await {
                                    if let Ok(chunk) = chunk {
                                        assistant.push_str(&chunk.message.content);
                                        let _ = tx_llm.send(Event::User(ChatEvent::Chunk(chunk)));
                                    }
                                }
                                let mut locked = msgs.lock().await;
                                locked.push(ChatMessage::assistant(assistant));
                            }
                        }
                    });
                }
                AppMsg::Quit => should_break = true,
                AppMsg::None => {}
            }
        }
        terminal.draw(|f| app.view(f, f.area()))?;
        if should_break {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
