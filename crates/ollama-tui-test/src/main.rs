use std::{io::stdout, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::Paragraph,
};
use tokio_stream::StreamExt;

use ollama_rs::{
    CoordinatorStreamEvent, Ollama, coordinator::Coordinator, generation::chat::ChatMessage,
};

/// Get the weather for a given city (mock implementation)
#[ollama_rs::function]
async fn get_weather(city: String) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    Ok(format!(
        "The weather in {} is sunny with a temperature of 72°F",
        city
    ))
}

/// Calculate distance between two cities (mock implementation)
#[ollama_rs::function]
async fn calculate_distance(
    from_city: String,
    to_city: String,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    Ok(format!(
        "The distance from {} to {} is approximately 250 miles",
        from_city, to_city
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    if let Err(err) = res {
        eprintln!("{}", err);
    }
    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ollama = Ollama::default();
    let history = vec![];
    let mut coordinator = Coordinator::new(ollama, "gpt-oss:20b".to_string(), history)
        .add_tool(get_weather)
        .add_tool(calculate_distance)
        .debug(false);

    let mut lines: Vec<String> = Vec::new();
    let mut input = String::new();

    loop {
        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
                .split(size);

            let paragraph = Paragraph::new(lines.join("\n"));
            f.render_widget(paragraph, chunks[0]);

            let input_widget = Paragraph::new(format!("> {}", input));
            f.render_widget(input_widget, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char(c) => input.push(c),
                    KeyCode::Backspace => {
                        input.pop();
                    }
                    KeyCode::Enter => {
                        let query = input.trim().to_string();
                        if query.is_empty() {
                            input.clear();
                            continue;
                        }
                        if query == "/quit" {
                            break;
                        }
                        lines.push(format!("📝 User: {}", query));
                        lines.push("🤖 Assistant: ".to_string());
                        input.clear();

                        let stream = coordinator
                            .chat_stream(vec![ChatMessage::user(query)])
                            .await?;
                        let mut stream = Box::pin(stream);
                        let mut current_line = String::new();

                        while let Some(event) = stream.next().await {
                            match event {
                                CoordinatorStreamEvent::ContentChunk(content) => {
                                    current_line.push_str(&content);
                                }
                                CoordinatorStreamEvent::ToolCallStarted { name, args } => {
                                    lines.push(current_line.clone());
                                    lines.push(format!(
                                        "🔧 [Calling tool: {} with args: {}]",
                                        name, args
                                    ));
                                    lines.push("🤖 Assistant: ".to_string());
                                    current_line.clear();
                                }
                                CoordinatorStreamEvent::ToolCallCompleted { name, result } => {
                                    lines.push(format!("✅ [Tool {} completed: {}]", name, result));
                                    lines.push("🤖 Assistant: ".to_string());
                                    current_line.clear();
                                }
                                CoordinatorStreamEvent::FinalContentChunk(content) => {
                                    current_line.push_str(&content);
                                }
                                CoordinatorStreamEvent::Done => {
                                    lines.push(current_line.clone());
                                    lines.push("─".repeat(80));
                                    current_line.clear();
                                    break;
                                }
                                CoordinatorStreamEvent::Error(err) => {
                                    lines.push(current_line.clone());
                                    lines.push(format!("❌ [Error: {}]", err));
                                    lines.push("─".repeat(80));
                                    current_line.clear();
                                    break;
                                }
                            }

                            terminal.draw(|f| {
                                let size = f.size();
                                let chunks = Layout::default()
                                    .direction(Direction::Vertical)
                                    .constraints(
                                        [Constraint::Min(1), Constraint::Length(3)].as_ref(),
                                    )
                                    .split(size);
                                let paragraph = Paragraph::new(lines.join("\n"));
                                f.render_widget(paragraph, chunks[0]);
                                let input_widget = Paragraph::new(format!("> {}", input));
                                f.render_widget(input_widget, chunks[1]);
                            })?;
                        }
                    }
                    KeyCode::Esc => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
