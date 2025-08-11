use std::{
    collections::HashMap,
    io::stdout,
    sync::Arc,
    time::{Duration, Instant},
};

use clap::Parser;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use llm::mcp::{McpContext, McpToolExecutor, load_mcp_servers};
use llm::tools::{self, ToolEvent, ToolExecutor};
use llm::{self, ChatMessage, ChatMessageRequest, Provider};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::task::JoinHandle;
use tokio_stream::{Stream, StreamExt};
use tui_input::{Input, InputRequest, backend::crossterm::EventHandler as _};

mod markdown;
mod ui;
use ui::{DrawState, HistoryItem, LineMapping, ThinkingStep, draw_ui};

fn ensure_assistant_item(items: &mut Vec<HistoryItem>) -> usize {
    if let Some(HistoryItem::Assistant(..)) = items.last() {
        return items.len() - 1;
    }
    items.push(HistoryItem::Assistant(String::new()));
    return items.len() - 1;
}

fn ensure_thinking_item(items: &mut Vec<HistoryItem>) -> usize {
    if let Some(HistoryItem::Thinking { .. }) = items.last() {
        return items.len() - 1;
    }
    items.push(HistoryItem::Thinking {
        steps: Vec::new(),
        collapsed: false,
        start: Instant::now(),
        duration: Duration::default(),
        done: false,
    });
    return items.len() - 1;
}

fn handle_tool_event(
    ev: ToolEvent,
    items: &mut Vec<HistoryItem>,
    pending_tools: &mut HashMap<usize, (usize, usize)>,
    current_line: &mut String,
) {
    match ev {
        ToolEvent::Chunk(chunk) => {
            if let Some(thinking) = chunk.message.thinking.as_ref() {
                let idx = ensure_thinking_item(items);
                if let HistoryItem::Thinking { steps, .. } = &mut items[idx] {
                    if let Some(ThinkingStep::Thought(t)) = steps.last_mut() {
                        t.push_str(thinking);
                    } else {
                        steps.push(ThinkingStep::Thought(thinking.to_string()));
                    }
                }
            }
            if let Some(content) = chunk.message.content.as_ref() {
                if !content.is_empty() {
                    current_line.push_str(content);
                    let assistant_index = ensure_assistant_item(items);
                    if let HistoryItem::Assistant(line) = &mut items[assistant_index] {
                        *line = current_line.clone();
                    }
                }
            }
            if chunk.done && pending_tools.is_empty() {
                for item in items.iter_mut().rev() {
                    match item {
                        HistoryItem::Separator => break,
                        HistoryItem::Thinking {
                            collapsed,
                            start,
                            duration,
                            done,
                            ..
                        } => {
                            *collapsed = true;
                            *duration = start.elapsed();
                            *done = true;
                        }
                        _ => {}
                    }
                }
                items.push(HistoryItem::Separator);
                current_line.clear();
            }
        }
        ToolEvent::ToolStarted { id, name, args } => {
            let idx = ensure_thinking_item(items);
            if let HistoryItem::Thinking { steps, .. } = &mut items[idx] {
                steps.push(ThinkingStep::ToolCall {
                    name: name.clone(),
                    args: args.to_string(),
                    result: String::new(),
                    success: true,
                    collapsed: true,
                });
                let step_idx = steps.len() - 1;
                pending_tools.insert(id, (idx, step_idx));
            }
        }
        ToolEvent::ToolResult { id, result, .. } => {
            if let Some((t_idx, s_idx)) = pending_tools.remove(&id) {
                if let HistoryItem::Thinking { steps, .. } = &mut items[t_idx] {
                    if let Some(ThinkingStep::ToolCall {
                        result: r, success, ..
                    }) = steps.get_mut(s_idx)
                    {
                        match result {
                            Ok(text) => *r = text,
                            Err(err) => {
                                *r = format!("Tool Failed: {}", err);
                                *success = false;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, value_enum, default_value_t = Provider::Ollama)]
    provider: Provider,
    /// Model identifier to use
    #[arg(long, default_value = "gpt-oss:20b")]
    model: String,
    /// LLM host URL, e.g. http://localhost:11434 for Ollama, https://api.openai.com/v1 for OpenAI or https://generativelanguage.googleapis.com for Gemini
    #[arg(long, default_value = "http://127.0.0.1:11434")]
    host: String,
    /// Path to MCP configuration JSON
    #[arg(long)]
    mcp: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();
    let (mcp_ctx, _services) = if let Some(path) = &args.mcp {
        load_mcp_servers(path).await?
    } else {
        (McpContext::default(), Vec::new())
    };
    let mcp_ctx = Arc::new(mcp_ctx);

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let client = llm::client_from(args.provider, &args.host)?;

    let res = run_app(&mut terminal, client, args.model.clone(), mcp_ctx.clone()).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;

    if let Err(err) = res {
        eprintln!("{}", err);
    }
    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    client: Arc<dyn llm::LlmClient>,
    model: String,
    mcp_ctx: Arc<McpContext>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tool_infos = { mcp_ctx.tool_infos.lock().await.clone() };
    let mut items: Vec<HistoryItem> = Vec::new();
    let mut input = Input::default();
    let mut chat_history: Vec<ChatMessage> = Vec::new();
    let mut scroll_offset: i32 = 0;
    let mut last_max_scroll: i32 = 0;
    let mut draw_state = DrawState::default();
    let mut events = EventStream::new();
    let mut current_line = String::new();
    let mut tool_stream: Option<Box<dyn Stream<Item = ToolEvent> + Unpin + Send>> = None;
    let mut pending_tools: HashMap<usize, (usize, usize)> = HashMap::new();
    let mut tool_task: Option<
        JoinHandle<Result<Vec<ChatMessage>, Box<dyn std::error::Error + Send + Sync>>>,
    > = None;
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(McpToolExecutor::new(mcp_ctx.clone()));

    loop {
        terminal.draw(|f| {
            draw_state = draw_ui(f, &items, &input, &mut scroll_offset, &mut last_max_scroll);
        })?;

        tokio::select! {
            maybe_event = events.next() => {
                if let Some(Ok(event)) = maybe_event {
                    match event {
                        Event::Key(key) => {
                            match (key.code, key.modifiers) {
                                (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => break,
                                (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => { input.reset(); }
                                (KeyCode::Char('j'), m) if m.contains(KeyModifiers::CONTROL) => {
                                    input.handle(InputRequest::InsertChar('\n'));
                                }
                                (KeyCode::Enter, _) => {
                                    let query = input.value().trim().to_string();
                                    if query.is_empty() {
                                        input.reset();
                                        continue;
                                    }
                                    if query == "/quit" { break; }
                                    if tool_task.is_some() { continue; }
                                    items.push(HistoryItem::User(query.clone()));
                                    input.reset();
                                    chat_history.push(ChatMessage::user(query.clone()));
                                    current_line.clear();
                                    let request = ChatMessageRequest::new(model.clone(), chat_history.clone())
                                        .tools(tool_infos.clone())
                                        .think(true);
                                    let history = std::mem::take(&mut chat_history);
                                    let client = client.clone();
                                    let exec = tool_executor.clone();
                                    let (stream, handle) =
                                        tools::tool_event_stream(client, request, exec, history);
                                    tool_stream = Some(Box::new(stream));
                                    tool_task = Some(handle);
                                }
                                (KeyCode::Esc, _) => break,
                                _ => { input.handle_event(&Event::Key(key)); },
                            }
                        }
                        Event::Paste(data) => {
                            for c in data.chars() { input.handle(InputRequest::InsertChar(c)); }
                        }
                        Event::Mouse(m) => match m.kind {
                            MouseEventKind::ScrollUp => scroll_offset += 1,
                            MouseEventKind::ScrollDown => scroll_offset -= 1,
                            MouseEventKind::Down(MouseButton::Left) => {
                                if m.column >= draw_state.history_rect.x
                                    && m.column < draw_state.history_rect.x + draw_state.history_rect.width
                                    && m.row >= draw_state.history_rect.y
                                    && m.row < draw_state.history_rect.y + draw_state.history_rect.height
                                {
                                    let idx = draw_state.top_line + (m.row - draw_state.history_rect.y) as usize;
                                    if let Some(map) = draw_state.line_map.get(idx) {
                                        match *map {
                                            LineMapping::Item(item_idx) => {
                                                if let Some(HistoryItem::Thinking { collapsed, done, .. }) = items.get_mut(item_idx) {
                                                    if *done { *collapsed = !*collapsed; }
                                                }
                                            }
                                            LineMapping::Step { item, step } => {
                                                if let Some(HistoryItem::Thinking { steps, .. }) = items.get_mut(item) {
                                                    if let Some(ThinkingStep::ToolCall { collapsed, .. }) = steps.get_mut(step) {
                                                        *collapsed = !*collapsed;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
            tool_event = async {
                if let Some(stream) = &mut tool_stream {
                    stream.next().await
                } else {
                    std::future::pending().await
                }
            }, if tool_stream.is_some() => {
                match tool_event {
                    Some(ev) => handle_tool_event(ev, &mut items, &mut pending_tools, &mut current_line),
                    None => { tool_stream = None; }
                }
            }
            res = async { if let Some(handle) = &mut tool_task { handle.await } else { std::future::pending().await } }, if tool_task.is_some() => {
                match res {
                    Ok(Ok(history)) => chat_history = history,
                    Ok(Err(err)) => items.push(HistoryItem::Error(err.to_string())),
                    Err(err) => items.push(HistoryItem::Error(err.to_string())),
                }
                if let Some(stream) = &mut tool_stream {
                    while let Some(ev) = stream.next().await {
                        handle_tool_event(ev, &mut items, &mut pending_tools, &mut current_line);
                    }
                }
                tool_stream = None;
                tool_task = None;
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }

    Ok(())
}
