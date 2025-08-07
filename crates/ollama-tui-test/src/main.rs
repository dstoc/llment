use std::{collections::HashMap, io::stdout, time::Duration};

use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ollama_rs::re_exports::schemars::Schema;
use ollama_rs::{
    Ollama,
    generation::chat::{ChatMessage, request::ChatMessageRequest},
    generation::tools::{ToolCall, ToolFunctionInfo, ToolInfo, ToolType},
};
use once_cell::sync::Lazy;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};
use rmcp::service::ServerSink;
use rmcp::{
    model::{CallToolRequestParam, RawContent},
    service::{RoleClient, RunningService, ServiceExt},
    transport::TokioChildProcess,
};
use serde::Deserialize;
use serde_json::Value;
use textwrap::wrap;
use tokio::{process::Command, sync::Mutex};
use tokio_stream::StreamExt;
use tui_markdown::from_str;

static MCP_TOOLS: Lazy<Mutex<HashMap<String, ServerSink>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static MCP_TOOL_INFOS: Lazy<Mutex<Vec<ToolInfo>>> = Lazy::new(|| Mutex::new(Vec::new()));

enum HistoryItem {
    Text(String),
    Thinking {
        text: String,
        collapsed: bool,
    },
    ToolCall {
        name: String,
        args: String,
        collapsed: bool,
    },
    ToolResult {
        name: String,
        result: String,
        success: bool,
        collapsed: bool,
    },
    Separator,
}

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

#[derive(Parser, Debug)]
struct Args {
    /// Ollama host URL, e.g. http://localhost:11434
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

fn wrap_history_lines(items: &[HistoryItem], width: usize) -> (Vec<String>, Vec<usize>) {
    let mut lines = Vec::new();
    let mut mapping = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let text = match item {
            HistoryItem::Text(t) => t.clone(),
            HistoryItem::Thinking { text, collapsed } => {
                if *collapsed {
                    "🤔 Thinking".to_string()
                } else {
                    format!("🤔 {}", text)
                }
            }
            HistoryItem::ToolCall {
                name,
                args,
                collapsed,
            } => {
                if *collapsed {
                    format!("🔧 {name}")
                } else {
                    format!("🔧 Calling tool: {name} with args: {args}")
                }
            }
            HistoryItem::ToolResult {
                name,
                result,
                success,
                collapsed,
            } => {
                let prefix = if *success { "✅" } else { "❌" };
                if *collapsed {
                    format!("{prefix} {name}")
                } else {
                    format!("{prefix} Tool {name} result: {result}")
                }
            }
            HistoryItem::Separator => "─".repeat(width),
        };
        let wrapped = wrap(&text, width.max(1));
        if wrapped.is_empty() {
            lines.push(String::new());
            mapping.push(idx);
        } else {
            for w in wrapped {
                lines.push(w.into_owned());
                mapping.push(idx);
            }
        }
    }
    (lines, mapping)
}

#[derive(Default)]
struct DrawState {
    history_rect: Rect,
    line_to_item: Vec<usize>,
    top_line: usize,
}

fn draw_ui(
    f: &mut Frame,
    items: &[HistoryItem],
    input: &str,
    scroll_offset: &mut i32,
) -> DrawState {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
        .split(area);

    let history_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(1)].as_ref())
        .split(chunks[0]);

    let width = history_chunks[0].width as usize;
    let (lines, mapping) = wrap_history_lines(items, width);
    let history_height = history_chunks[0].height as usize;
    let line_count = lines.len();
    let max_scroll = line_count.saturating_sub(history_height) as i32;
    *scroll_offset = (*scroll_offset).clamp(0, max_scroll);
    let top_line = (max_scroll - *scroll_offset) as usize;

    let content = lines.join("\n");
    let markdown = from_str(&content);
    let paragraph = Paragraph::new(markdown)
        .wrap(Wrap { trim: false })
        .scroll((top_line as u16, 0));
    f.render_widget(paragraph, history_chunks[0]);

    let mut scrollbar_state = ScrollbarState::new(line_count)
        .position(top_line)
        .viewport_content_length(history_height);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    f.render_stateful_widget(scrollbar, history_chunks[1], &mut scrollbar_state);

    let input_widget = Paragraph::new(format!("> {}", input));
    f.render_widget(input_widget, chunks[1]);

    DrawState {
        history_rect: history_chunks[0],
        line_to_item: mapping,
        top_line,
    }
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    host: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ollama = Ollama::try_new(host)?;
    let tool_infos = { MCP_TOOL_INFOS.lock().await.clone() };
    let mut items: Vec<HistoryItem> = Vec::new();
    let mut input = String::new();
    let mut chat_history: Vec<ChatMessage> = Vec::new();
    let mut scroll_offset: i32 = 0;
    let mut draw_state = DrawState::default();

    loop {
        terminal.draw(|f| {
            draw_state = draw_ui(f, &items, &input, &mut scroll_offset);
        })?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => match key.code {
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
                        items.push(HistoryItem::Text(format!("📝 User: {}", query)));
                        input.clear();
                        chat_history.push(ChatMessage::user(query.clone()));

                        loop {
                            items.push(HistoryItem::Text("🤖 Assistant: ".to_string()));
                            let mut assistant_index = items.len() - 1;
                            let mut current_line = String::new();
                            let mut current_thinking = String::new();
                            let mut thinking_index: Option<usize> = None;
                            let mut tool_calls: Vec<ToolCall> = Vec::new();

                            let request = ChatMessageRequest::new(
                                "gpt-oss:20b".to_string(),
                                chat_history.clone(),
                            )
                            .tools(tool_infos.clone())
                            .think(true);
                            let mut stream = ollama.send_chat_messages_stream(request).await?;
                            while let Some(chunk) = stream.next().await {
                                let chunk = match chunk {
                                    Ok(c) => c,
                                    Err(_) => break,
                                };
                                if let Some(thinking) = chunk.message.thinking.as_ref() {
                                    let idx = thinking_index.unwrap_or_else(|| {
                                        items.insert(
                                            assistant_index,
                                            HistoryItem::Thinking {
                                                text: String::new(),
                                                collapsed: false,
                                            },
                                        );
                                        let idx = assistant_index;
                                        assistant_index += 1;
                                        idx
                                    });
                                    thinking_index = Some(idx);
                                    current_thinking.push_str(thinking);
                                    if let HistoryItem::Thinking { text, .. } = &mut items[idx] {
                                        *text = current_thinking.clone();
                                    }
                                }
                                if !chunk.message.content.is_empty() {
                                    current_line.push_str(&chunk.message.content);
                                    if let HistoryItem::Text(line) = &mut items[assistant_index] {
                                        *line = format!("🤖 Assistant: {}", current_line);
                                    }
                                }
                                if chunk.done {
                                    tool_calls = chunk.message.tool_calls.clone();
                                    break;
                                }
                                terminal.draw(|f| {
                                    draw_state = draw_ui(f, &items, &input, &mut scroll_offset);
                                })?;
                            }

                            if let Some(idx) = thinking_index {
                                if let HistoryItem::Thinking { text, collapsed } = &mut items[idx] {
                                    *text = current_thinking.clone();
                                    *collapsed = true;
                                }
                            }

                            if !current_line.is_empty() {
                                chat_history.push(ChatMessage::assistant(current_line.clone()));
                            } else {
                                items.remove(assistant_index);
                            }

                            if tool_calls.is_empty() {
                                items.push(HistoryItem::Separator);
                                break;
                            }

                            for call in tool_calls {
                                items.push(HistoryItem::ToolCall {
                                    name: call.function.name.clone(),
                                    args: call.function.arguments.to_string(),
                                    collapsed: true,
                                });
                                let result = match call_mcp_tool(
                                    &call.function.name,
                                    call.function.arguments.clone(),
                                )
                                .await
                                {
                                    Ok(res) => {
                                        items.push(HistoryItem::ToolResult {
                                            name: call.function.name.clone(),
                                            result: res.clone(),
                                            success: true,
                                            collapsed: true,
                                        });
                                        res
                                    }
                                    Err(err) => {
                                        let err_str = format!("Tool Failed: {}", err);
                                        items.push(HistoryItem::ToolResult {
                                            name: call.function.name.clone(),
                                            result: err_str.clone(),
                                            success: false,
                                            collapsed: true,
                                        });
                                        err_str
                                    }
                                };
                                chat_history.push(ChatMessage::tool(
                                    result.clone(),
                                    call.function.name.clone(),
                                ));
                            }
                        }
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
                            let idx =
                                draw_state.top_line + (m.row - draw_state.history_rect.y) as usize;
                            if let Some(&item_idx) = draw_state.line_to_item.get(idx) {
                                if let Some(item) = items.get_mut(item_idx) {
                                    match item {
                                        HistoryItem::Thinking { collapsed, .. }
                                        | HistoryItem::ToolCall { collapsed, .. }
                                        | HistoryItem::ToolResult { collapsed, .. } => {
                                            *collapsed = !*collapsed;
                                        }
                                        _ => {}
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

    Ok(())
}
