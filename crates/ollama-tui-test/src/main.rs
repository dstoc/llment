use std::{
    collections::HashMap,
    io::stdout,
    sync::Arc,
    time::{Duration, Instant},
};

use clap::{Parser, ValueEnum};
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use llm_core::{
    ChatMessage, ChatMessageRequest, ChatStream, LlmClient, Schema, ToolCall, ToolFunctionInfo,
    ToolInfo, ToolType,
};
use once_cell::sync::Lazy;
use ratatui::{Terminal, backend::CrosstermBackend};
use rmcp::service::ServerSink;
use rmcp::{
    model::{CallToolRequestParam, RawContent},
    service::{RoleClient, RunningService, ServiceExt},
    transport::TokioChildProcess,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::{process::Command, sync::Mutex, task::JoinSet};
use tokio_stream::StreamExt;
use tui_input::{Input, InputRequest, backend::crossterm::EventHandler as _};

mod markdown;
mod ui;
use ui::{DrawState, HistoryItem, LineMapping, ThinkingStep, draw_ui};

static MCP_TOOLS: Lazy<Mutex<HashMap<String, ServerSink>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static MCP_TOOL_INFOS: Lazy<Mutex<Vec<ToolInfo>>> = Lazy::new(|| Mutex::new(Vec::new()));

#[derive(Deserialize)]
struct McpConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: HashMap<String, McpServer>,
}

#[derive(Deserialize)]
struct McpServer {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

async fn load_mcp_servers(
    path: &str,
) -> Result<Vec<RunningService<RoleClient, ()>>, Box<dyn std::error::Error + Send + Sync>> {
    let data = tokio::fs::read_to_string(path).await?;
    let config: McpConfig = serde_json::from_str(&data)?;
    let mut services = Vec::new();
    for server in config.mcp_servers.values() {
        let mut cmd = Command::new(&server.command);
        cmd.args(&server.args);
        for (k, v) in &server.env {
            cmd.env(k, v);
        }
        let process = TokioChildProcess::new(cmd)?;
        let service = ().serve(process).await?;
        let tools = service.list_tools(Default::default()).await?;
        {
            let mut map = MCP_TOOLS.lock().await;
            let mut infos = MCP_TOOL_INFOS.lock().await;
            for tool in tools.tools {
                map.insert(tool.name.to_string(), service.peer().clone());
                let schema: Schema = serde_json::from_value(tool.schema_as_json_value())?;
                let description = tool.description.clone().unwrap_or_default().to_string();
                infos.push(ToolInfo {
                    tool_type: ToolType::Function,
                    function: ToolFunctionInfo {
                        name: tool.name.to_string(),
                        description,
                        parameters: schema,
                    },
                });
            }
        }
        services.push(service);
    }
    Ok(services)
}

async fn call_mcp_tool(
    name: &str,
    args: Value,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let peer = {
        let map = MCP_TOOLS.lock().await;
        map.get(name).cloned()
    }
    .ok_or_else(|| format!("Tool {name} not found"))?;

    let result = peer
        .call_tool(CallToolRequestParam {
            name: name.to_string().into(),
            arguments: args.as_object().cloned(),
        })
        .await?;

    if let Some(content) = result.content {
        let text = content
            .into_iter()
            .filter_map(|c| match c.raw {
                RawContent::Text(t) => Some(t.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(text)
    } else if let Some(value) = result.structured_content {
        Ok(value.to_string())
    } else {
        Ok(String::new())
    }
}

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

fn spawn_tool_call(
    call: ToolCall,
    items: &mut Vec<HistoryItem>,
    thinking_idx: usize,
    handles: &mut JoinSet<(
        usize,
        usize,
        String,
        Result<String, Box<dyn std::error::Error + Send + Sync>>,
    )>,
) {
    if let HistoryItem::Thinking { steps, .. } = &mut items[thinking_idx] {
        steps.push(ThinkingStep::ToolCall {
            name: call.function.name.clone(),
            args: call.function.arguments.to_string(),
            result: String::new(),
            success: true,
            collapsed: true,
        });
        let step_idx = steps.len() - 1;
        let name = call.function.name.clone();
        let args = call.function.arguments.clone();
        handles.spawn(async move {
            let res = call_mcp_tool(&name, args).await;
            (thinking_idx, step_idx, name, res)
        });
    }
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Provider {
    Ollama,
    Openai,
    Gemini,
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

    let _services = if let Some(path) = &args.mcp {
        load_mcp_servers(path).await?
    } else {
        Vec::new()
    };

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

    let client: Arc<dyn LlmClient> = match args.provider {
        Provider::Ollama => Arc::new(llm_core::ollama::OllamaClient::new(&args.host)?),
        Provider::Openai => Arc::new(llm_core::openai::OpenAiClient::new(&args.host)),
        Provider::Gemini => Arc::new(llm_core::gemini::GeminiClient::new(&args.host)),
    };

    let res = run_app(&mut terminal, client, args.model.clone()).await;

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
    client: Arc<dyn LlmClient>,
    model: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tool_infos = { MCP_TOOL_INFOS.lock().await.clone() };
    let mut items: Vec<HistoryItem> = Vec::new();
    let mut input = Input::default();
    let mut chat_history: Vec<ChatMessage> = Vec::new();
    let mut scroll_offset: i32 = 0;
    let mut last_max_scroll: i32 = 0;
    let mut draw_state = DrawState::default();
    let mut events = EventStream::new();
    let mut chat_stream: Option<ChatStream> = None;
    let mut current_line = String::new();
    let mut tool_handles: JoinSet<(
        usize,
        usize,
        String,
        Result<String, Box<dyn std::error::Error + Send + Sync>>,
    )> = JoinSet::new();
    let mut saw_tool_call = false;
    let mut request_done = false;

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
                                (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => {
                                    input.reset();
                                }
                                (KeyCode::Char('j'), m) if m.contains(KeyModifiers::CONTROL) => {
                                    input.handle(InputRequest::InsertChar('\n'));
                                }
                                (KeyCode::Enter, _) => {
                                    let query = input.value().trim().to_string();
                                    if query.is_empty() {
                                        input.reset();
                                        continue;
                                    }
                                    if query == "/quit" {
                                        break;
                                    }
                                    if chat_stream.is_some() || !tool_handles.is_empty() {
                                        continue;
                                    }
                                    items.push(HistoryItem::User(query.clone()));
                                    input.reset();
                                    chat_history.push(ChatMessage::user(query.clone()));
                                    current_line.clear();
                                    let request = ChatMessageRequest::new(
                                        model.clone(),
                                        chat_history.clone(),
                                    )
                                    .tools(tool_infos.clone())
                                    .think(true);
                                    chat_stream = Some(client.send_chat_messages_stream(request).await?);
                                }
                                (KeyCode::Esc, _) => break,
                                _ => {
                                        input.handle_event(&Event::Key(key));
                                }
                            }
                        }
                        Event::Paste(data) => {
                            for c in data.chars() {
                                input.handle(InputRequest::InsertChar(c));
                            }
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
            chat_chunk = async {
                if let Some(stream) = &mut chat_stream {
                    stream.next().await
                } else {
                    None
                }
            }, if chat_stream.is_some() => {
                if let Some(Ok(chunk)) = chat_chunk {
                    if let Some(thinking) = chunk.message.thinking.as_ref() {
                        let idx = ensure_thinking_item(&mut items);
                        if let HistoryItem::Thinking { steps, .. } = &mut items[idx] {
                            if let Some(ThinkingStep::Thought(t)) = steps.last_mut() {
                                t.push_str(thinking);
                            } else {
                                steps.push(ThinkingStep::Thought(thinking.to_string()));
                            }
                        }
                    }
                    if !chunk.message.content.is_empty() {
                        current_line.push_str(&chunk.message.content);
                        let assistant_index = ensure_assistant_item(&mut items);
                        if let HistoryItem::Assistant(line) = &mut items[assistant_index] {
                            *line = current_line.clone();
                        }
                    }
                    if !chunk.message.tool_calls.is_empty() {
                        let idx = ensure_thinking_item(&mut items);
                        for call in chunk.message.tool_calls {
                            spawn_tool_call(call, &mut items, idx, &mut tool_handles);
                            saw_tool_call = true;
                        }
                    }
                    if chunk.done {
                        chat_stream = None;
                        request_done = true;

                        if !saw_tool_call {
                            // Collapse all thinking blocks part of this assistant flow
                            for item in items.iter_mut().rev() {
                                match item {
                                    HistoryItem::Separator => break,
                                    HistoryItem::Thinking { collapsed, start, duration, done, .. } => {
                                        *collapsed = true;
                                        // TODO: should we rather update this when we complete the tool calls?
                                        *duration = start.elapsed();
                                        *done = true;
                                    }
                                    _ => {}
                                }
                            }
                            items.push(HistoryItem::Separator);
                            request_done = false;
                        } else if tool_handles.is_empty() {
                            current_line.clear();
                            let request = ChatMessageRequest::new(
                                model.clone(),
                                chat_history.clone(),
                            )
                            .tools(tool_infos.clone())
                            .think(true);
                            chat_stream = Some(client.send_chat_messages_stream(request).await?);
                            saw_tool_call = false;
                            request_done = false;
                        }
                    }
                } else if let Some(Err(msg)) = chat_chunk {
                    // TODO: remove when we validate Error history items
                    println!("{:}", msg.to_string());
                    items.push(HistoryItem::Error(msg.to_string()));
                    chat_stream = None;
                    request_done = false;
                }
            }
            tool_res = tool_handles.join_next(), if !tool_handles.is_empty() => {
                if let Some(Ok((t_idx, s_idx, name, res))) = tool_res {
                    if let HistoryItem::Thinking { steps, .. } = &mut items[t_idx] {
                        if let Some(ThinkingStep::ToolCall { result, success, .. }) = steps.get_mut(s_idx) {
                            match res {
                                Ok(text) => {
                                    *result = text.clone();
                                    chat_history.push(ChatMessage::tool(text, name));
                                }
                                Err(err) => {
                                    *result = format!("Tool Failed: {}", err);
                                    chat_history.push(ChatMessage::tool(result.clone(), name));
                                    *success = false;
                                }
                            }
                        }
                    }
                }
                if tool_handles.is_empty() && request_done && saw_tool_call {
                    current_line.clear();
                    let request = ChatMessageRequest::new(
                        model.clone(),
                        chat_history.clone(),
                    )
                    .tools(tool_infos.clone())
                    .think(true);
                    chat_stream = Some(client.send_chat_messages_stream(request).await?);
                    saw_tool_call = false;
                    request_done = false;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }

    Ok(())
}
