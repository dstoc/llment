use std::{
    collections::{HashMap, VecDeque},
    io::stdout,
    time::{Duration, Instant},
};

use clap::Parser;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, MouseButton,
        MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use genai::adapter::AdapterKind;
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest, ChatStream, ChatStreamEvent, MessageContent, Tool,
    ToolCall, ToolResponse,
};
use genai::resolver::{Endpoint, ServiceTargetResolver};
use genai::{Client, ModelIden, ServiceTarget};
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
use tokio::{process::Command, sync::Mutex, task::JoinHandle};
use tokio_stream::StreamExt;

mod markdown;
mod ui;
use ui::{DrawState, HistoryItem, LineMapping, ThinkingStep, draw_ui};

static MCP_TOOLS: Lazy<Mutex<HashMap<String, ServerSink>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static MCP_TOOL_INFOS: Lazy<Mutex<Vec<Tool>>> = Lazy::new(|| Mutex::new(Vec::new()));

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
                let schema = tool.schema_as_json_value();
                let description = tool.description.clone().unwrap_or_default().to_string();
                infos.push(
                    Tool::new(tool.name.to_string())
                        .with_description(description)
                        .with_schema(schema),
                );
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

fn start_next_tool_call(
    pending: &mut VecDeque<ToolCall>,
    items: &mut Vec<HistoryItem>,
    thinking_index: Option<usize>,
) -> Option<
    JoinHandle<(
        usize,
        String,
        String,
        Result<String, Box<dyn std::error::Error + Send + Sync>>,
    )>,
> {
    let idx = thinking_index?;
    if let Some(call) = pending.pop_front() {
        if let HistoryItem::Thinking { steps, .. } = &mut items[idx] {
            steps.push(ThinkingStep::ToolCall {
                name: call.fn_name.clone(),
                args: call.fn_arguments.to_string(),
                result: String::new(),
                success: true,
                collapsed: true,
            });
            let step_idx = steps.len() - 1;
            let name = call.fn_name.clone();
            let call_id = call.call_id.clone();
            let args = call.fn_arguments.clone();
            return Some(tokio::spawn(async move {
                let res = call_mcp_tool(&name, args).await;
                (step_idx, name, call_id, res)
            }));
        }
    }
    None
}

#[derive(Parser, Debug)]
struct Args {
    /// Provider name (e.g. ollama, openai)
    #[arg(long, default_value = "ollama")]
    provider: String,
    /// Model name
    #[arg(long, default_value = "gpt-oss:20b")]
    model: String,
    /// Endpoint base URL, e.g. http://localhost:11434/v1/
    #[arg(long, default_value = "http://127.0.0.1:11434/v1/")]
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
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, &args.provider, &args.model, &args.host).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    if let Err(err) = res {
        eprintln!("{}", err);
    }
    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    provider: &str,
    model: &str,
    endpoint: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let provider_kind = match provider.to_lowercase().as_str() {
        "openai" => AdapterKind::OpenAI,
        "anthropic" => AdapterKind::Anthropic,
        "cohere" => AdapterKind::Cohere,
        "gemini" => AdapterKind::Gemini,
        "groq" => AdapterKind::Groq,
        "xai" => AdapterKind::Xai,
        "deepseek" => AdapterKind::DeepSeek,
        _ => AdapterKind::Ollama,
    };
    let model_owned = model.to_string();
    let mut endpoint_owned = endpoint.to_string();
    if !endpoint_owned.ends_with('/') {
        endpoint_owned.push('/');
    }
    if !endpoint_owned.ends_with("v1/") {
        endpoint_owned.push_str("v1/");
    }
    let resolver = ServiceTargetResolver::from_resolver_fn(move |mut st: ServiceTarget| {
        st.endpoint = Endpoint::from_owned(endpoint_owned.clone());
        Ok(st)
    });
    let client = Client::builder()
        .with_model_mapper_fn(move |_m: ModelIden| {
            Ok(ModelIden::new(provider_kind, model_owned.clone()))
        })
        .with_service_target_resolver(resolver)
        .build();
    let tool_infos = { MCP_TOOL_INFOS.lock().await.clone() };
    let mut items: Vec<HistoryItem> = Vec::new();
    let mut input = String::new();
    let mut chat_history: Vec<ChatMessage> = Vec::new();
    let mut scroll_offset: i32 = 0;
    let mut draw_state = DrawState::default();
    let mut events = EventStream::new();
    let mut chat_stream: Option<ChatStream> = None;
    let mut thinking_index: Option<usize> = None;
    let mut assistant_index: usize = 0;
    let mut current_line = String::new();
    let mut pending_tool_calls: VecDeque<ToolCall> = VecDeque::new();
    let mut tool_handle: Option<
        JoinHandle<(
            usize,
            String,
            String,
            Result<String, Box<dyn std::error::Error + Send + Sync>>,
        )>,
    > = None;

    loop {
        terminal.draw(|f| {
            draw_state = draw_ui(f, &items, &input, &mut scroll_offset);
        })?;

        tokio::select! {
            maybe_event = events.next() => {
                if let Some(Ok(event)) = maybe_event {
                    match event {
                        Event::Key(key) => match key.code {
                            KeyCode::Char(c) => input.push(c),
                            KeyCode::Backspace => { input.pop(); }
                            KeyCode::Enter => {
                                let query = input.trim().to_string();
                                if query.is_empty() {
                                    input.clear();
                                    continue;
                                }
                                if query == "/quit" {
                                    break;
                                }
                                if chat_stream.is_some() || tool_handle.is_some() {
                                    continue;
                                }
                                items.push(HistoryItem::User(query.clone()));
                                input.clear();
                                chat_history.push(ChatMessage::user(query.clone()));
                                items.push(HistoryItem::Assistant(String::new()));
                                assistant_index = items.len() - 1;
                                current_line.clear();
                                thinking_index = None;
                                let request = ChatRequest::from_messages(chat_history.clone())
                                    .with_tools(tool_infos.clone());
                                let options = ChatOptions::default()
                                    .with_capture_content(true)
                                    .with_capture_reasoning_content(true)
                                    .with_normalize_reasoning_content(true);
                                let stream_res = client
                                    .exec_chat_stream(model, request, Some(&options))
                                    .await?;
                                chat_stream = Some(stream_res.stream);
                            }
                            KeyCode::Esc => break,
                            _ => {}
                        },
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
            chat_event = async {
                if let Some(stream) = &mut chat_stream {
                    stream.next().await
                } else {
                    None
                }
            }, if chat_stream.is_some() => {
                if let Some(Ok(event)) = chat_event {
                    match event {
                        ChatStreamEvent::ReasoningChunk(chunk) => {
                            let idx = thinking_index.unwrap_or_else(|| {
                                items.insert(
                                    assistant_index,
                                    HistoryItem::Thinking {
                                        steps: Vec::new(),
                                        collapsed: false,
                                        start: Instant::now(),
                                        duration: Duration::default(),
                                        done: false,
                                    },
                                );
                                let idx = assistant_index;
                                assistant_index += 1;
                                idx
                            });
                            thinking_index = Some(idx);
                            if let HistoryItem::Thinking { steps, .. } = &mut items[idx] {
                                if let Some(ThinkingStep::Thought(t)) = steps.last_mut() {
                                    t.push_str(&chunk.content);
                                } else {
                                    steps.push(ThinkingStep::Thought(chunk.content));
                                }
                            }
                        }
                        ChatStreamEvent::Chunk(chunk) => {
                            current_line.push_str(&chunk.content);
                            if let HistoryItem::Assistant(line) = &mut items[assistant_index] {
                                *line = current_line.clone();
                            }
                        }
                        ChatStreamEvent::End(end) => {
                            chat_stream = None;
                            let reasoning = end.captured_reasoning_content.clone();
                            let has_tool_calls = if let Some(MessageContent::ToolCalls(calls)) = end.captured_content.clone() {
                                pending_tool_calls = VecDeque::from(calls.clone());
                                chat_history.push(ChatMessage::from(calls));
                                true
                            } else if !current_line.is_empty() {
                                chat_history.push(ChatMessage::assistant(current_line.clone()));
                                false
                            } else {
                                items.remove(assistant_index);
                                false
                            };

                            if thinking_index.is_none() && (has_tool_calls || reasoning.is_some()) {
                                items.insert(
                                    assistant_index,
                                    HistoryItem::Thinking {
                                        steps: Vec::new(),
                                        collapsed: false,
                                        start: Instant::now(),
                                        duration: Duration::default(),
                                        done: false,
                                    },
                                );
                                thinking_index = Some(assistant_index);
                                assistant_index += 1;
                            }

                            if let Some(reason) = reasoning {
                                if let Some(idx) = thinking_index {
                                    if let HistoryItem::Thinking { steps, .. } = &mut items[idx] {
                                        if let Some(ThinkingStep::Thought(t)) = steps.last_mut() {
                                            t.push_str(&reason);
                                        } else {
                                            steps.push(ThinkingStep::Thought(reason));
                                        }
                                    }
                                }
                            }

                            if pending_tool_calls.is_empty() {
                                if let Some(idx) = thinking_index {
                                    if let HistoryItem::Thinking { collapsed, start, duration, done, .. } = &mut items[idx] {
                                        *collapsed = true;
                                        *duration = start.elapsed();
                                        *done = true;
                                    }
                                }
                                items.push(HistoryItem::Separator);
                            } else {
                                tool_handle = start_next_tool_call(&mut pending_tool_calls, &mut items, thinking_index);
                            }
                        }
                        ChatStreamEvent::Start => {}
                    }
                } else {
                    chat_stream = None;
                }
            }
            tool_res = async {
                if let Some(handle) = &mut tool_handle {
                    Some(handle.await)
                } else {
                    None
                }
            }, if tool_handle.is_some() => {
                if let Some(Ok((s_idx, _name, call_id, res))) = tool_res {
                    if let Some(t_idx) = thinking_index {
                        if let HistoryItem::Thinking { steps, .. } = &mut items[t_idx] {
                            if let Some(ThinkingStep::ToolCall { result, success, .. }) = steps.get_mut(s_idx) {
                                match res {
                                    Ok(text) => {
                                        *result = text.clone();
                                        chat_history.push(ChatMessage::from(ToolResponse::new(call_id, text)));
                                    }
                                    Err(err) => {
                                        *result = format!("Tool Failed: {}", err);
                                        *success = false;
                                    }
                                }
                            }
                        }
                    }
                    tool_handle = start_next_tool_call(&mut pending_tool_calls, &mut items, thinking_index);
                    if tool_handle.is_none() {
                        items.push(HistoryItem::Assistant(String::new()));
                        assistant_index = items.len() - 1;
                        current_line.clear();
                        let request = ChatRequest::from_messages(chat_history.clone())
                            .with_tools(tool_infos.clone());
                        let options = ChatOptions::default()
                            .with_capture_content(true)
                            .with_capture_reasoning_content(true)
                            .with_normalize_reasoning_content(true);
                        let stream_res = client
                            .exec_chat_stream(model, request, Some(&options))
                            .await?;
                        chat_stream = Some(stream_res.stream);
                    }
                } else {
                    tool_handle = None;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }

    Ok(())
}
