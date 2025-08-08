use std::{
    collections::{HashMap, VecDeque},
    io::stdout,
    time::{Duration, Instant},
};

use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessageArgs,
        ChatCompletionRequestMessage, ChatCompletionRequestToolMessageArgs,
        ChatCompletionRequestUserMessageArgs, ChatCompletionTool, ChatCompletionToolType,
        CreateChatCompletionRequestArgs, FunctionObject,
    },
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

static MCP_TOOL_INFOS: Lazy<Mutex<Vec<ChatCompletionTool>>> = Lazy::new(|| Mutex::new(Vec::new()));

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
                infos.push(ChatCompletionTool {
                    r#type: ChatCompletionToolType::Function,
                    function: FunctionObject {
                        name: tool.name.to_string(),
                        description: tool.description.clone().map(|d| d.to_string()),
                        parameters: Some(tool.schema_as_json_value()),
                        strict: Some(false),
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

fn start_next_tool_call(
    pending: &mut VecDeque<ChatCompletionMessageToolCall>,
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
    if let Some(call) = pending.pop_front() {
        if let Some(idx) = thinking_index {
            if let HistoryItem::Thinking { steps, .. } = &mut items[idx] {
                steps.push(ThinkingStep::ToolCall {
                    name: call.function.name.clone(),
                    args: call.function.arguments.clone(),
                    result: String::new(),
                    success: true,
                    collapsed: true,
                });
                let step_idx = steps.len() - 1;
                let id = call.id.clone();
                let name = call.function.name.clone();
                let args_str = call.function.arguments.clone();
                return Some(tokio::spawn(async move {
                    let args: Value = serde_json::from_str(&args_str).unwrap_or(Value::Null);
                    let res = call_mcp_tool(&name, args).await;
                    (step_idx, id, name, res)
                }));
            }
        }
    }
    None
}

#[derive(Parser, Debug)]
struct Args {
    /// OpenAI API base URL, e.g. http://localhost:11434
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
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, &args.host).await;

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
    host: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = OpenAIConfig::new().with_api_base(host);
    let client = Client::with_config(config);
    let tool_infos = { MCP_TOOL_INFOS.lock().await.clone() };
    let mut items: Vec<HistoryItem> = Vec::new();
    let mut input = String::new();
    let mut chat_history: Vec<ChatCompletionRequestMessage> = Vec::new();
    let mut scroll_offset: i32 = 0;
    let mut draw_state = DrawState::default();
    let mut events = EventStream::new();
    let mut chat_task: Option<
        JoinHandle<
            Result<
                (String, Vec<ChatCompletionMessageToolCall>),
                Box<dyn std::error::Error + Send + Sync>,
            >,
        >,
    > = None;
    let mut thinking_index: Option<usize> = None;
    let mut assistant_index: usize = 0;
    let mut current_line = String::new();
    let mut pending_tool_calls: VecDeque<ChatCompletionMessageToolCall> = VecDeque::new();
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
                                if chat_task.is_some() || tool_handle.is_some() {
                                    continue;
                                }
                                items.push(HistoryItem::User(query.clone()));
                                input.clear();
                                chat_history.push(
                                    ChatCompletionRequestUserMessageArgs::default()
                                        .content(query.clone())
                                        .build()?
                                        .into(),
                                );
                                items.push(HistoryItem::Assistant(String::new()));
                                assistant_index = items.len() - 1;
                                current_line.clear();
                                thinking_index = None;
                                let client_clone = client.clone();
                                let messages = chat_history.clone();
                                let tools = tool_infos.clone();
                                chat_task = Some(tokio::spawn(async move {
                                    let request = CreateChatCompletionRequestArgs::default()
                                        .model("gpt-4o-mini")
                                        .messages(messages)
                                        .tools(tools)
                                        .build()?;
                                    let resp = client_clone.chat().create(request).await?;
                                    let choice = resp
                                        .choices
                                        .into_iter()
                                        .next()
                                        .ok_or("no choices")?;
                                    let content = choice.message.content.unwrap_or_default();
                                    let tool_calls = choice.message.tool_calls.unwrap_or_default();
                                    Ok((content, tool_calls))
                                }));
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
            chat_res = async {
                if let Some(handle) = &mut chat_task {
                    Some(handle.await)
                } else {
                    None
                }
            }, if chat_task.is_some() => {
                if let Some(Ok(Ok((content, tool_calls)))) = chat_res {
                    current_line.push_str(&content);
                    if let HistoryItem::Assistant(line) = &mut items[assistant_index] {
                        *line = current_line.clone();
                    }
                    pending_tool_calls = VecDeque::from(tool_calls.clone());
                    if !pending_tool_calls.is_empty() && thinking_index.is_none() {
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
                    if !current_line.is_empty() {
                        chat_history.push(
                            ChatCompletionRequestAssistantMessageArgs::default()
                                .content(current_line.clone())
                                .tool_calls(tool_calls)
                                .build()?
                                .into(),
                        );
                    } else {
                        items.remove(assistant_index);
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
                chat_task = None;
            }
            tool_res = async {
                if let Some(handle) = &mut tool_handle {
                    Some(handle.await)
                } else {
                    None
                }
            }, if tool_handle.is_some() => {
                if let Some(Ok((s_idx, id, _name, res))) = tool_res {
                    if let Some(t_idx) = thinking_index {
                        if let HistoryItem::Thinking { steps, .. } = &mut items[t_idx] {
                            if let Some(ThinkingStep::ToolCall { result, success, .. }) = steps.get_mut(s_idx) {
                                match res {
                                    Ok(text) => {
                                        *result = text.clone();
                                        chat_history.push(
                                            ChatCompletionRequestToolMessageArgs::default()
                                                .tool_call_id(id.clone())
                                                .content(text.clone())
                                                .build()?
                                                .into(),
                                        );
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
                        let client_clone = client.clone();
                        let messages = chat_history.clone();
                        let tools = tool_infos.clone();
                        chat_task = Some(tokio::spawn(async move {
                            let request = CreateChatCompletionRequestArgs::default()
                                .model("gpt-4o-mini")
                                .messages(messages)
                                .tools(tools)
                                .build()?;
                            let resp = client_clone.chat().create(request).await?;
                            let choice = resp
                                .choices
                                .into_iter()
                                .next()
                                .ok_or("no choices")?;
                            let content = choice.message.content.unwrap_or_default();
                            let tool_calls = choice.message.tool_calls.unwrap_or_default();
                            Ok((content, tool_calls))
                        }));
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
