use std::{collections::HashMap, io::stdout, time::Duration};

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use once_cell::sync::Lazy;
use ollama_rs::{
    generation::chat::{request::ChatMessageRequest, ChatMessage},
    generation::tools::{ToolCall, ToolFunctionInfo, ToolInfo, ToolType},
    Ollama,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::Paragraph,
    Terminal,
};
use rmcp::{
    model::{CallToolRequestParam, RawContent},
    service::{RunningService, RoleClient, ServiceExt},
    transport::TokioChildProcess,
};
use rmcp::service::ServerSink;
use ollama_rs::re_exports::schemars::Schema;
use serde::Deserialize;
use tokio::{process::Command, sync::Mutex};
use tokio_stream::StreamExt;
use serde_json::Value;

static MCP_TOOLS: Lazy<Mutex<HashMap<String, ServerSink>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static MCP_TOOL_INFOS: Lazy<Mutex<Vec<ToolInfo>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

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
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, &args.host).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    if let Err(err) = res {
        eprintln!("{}", err);
    }
    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    host: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ollama = Ollama::try_new(host)?;
    let tool_infos = { MCP_TOOL_INFOS.lock().await.clone() };

    let mut lines: Vec<String> = Vec::new();
    let mut input = String::new();
    let mut history: Vec<ChatMessage> = Vec::new();

    loop {
        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
                .split(area);

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

                        history.push(ChatMessage::user(query.clone()));
                        let mut current_line = String::new();
                        let mut tool_calls: Vec<ToolCall> = Vec::new();
                        {
                            let request = ChatMessageRequest::new(
                                "gpt-oss:20b".to_string(),
                                history.clone(),
                            )
                            .tools(tool_infos.clone());
                            let mut stream =
                                ollama.send_chat_messages_stream(request).await?;
                            while let Some(chunk) = stream.next().await {
                                let chunk = match chunk {
                                    Ok(c) => c,
                                    Err(_) => break,
                                };
                                if !chunk.message.content.is_empty() {
                                    current_line.push_str(&chunk.message.content);
                                }
                                if chunk.done {
                                    tool_calls = chunk.message.tool_calls.clone();
                                    break;
                                }
                                terminal.draw(|f| {
                                    let area = f.area();
                                    let chunks = Layout::default()
                                        .direction(Direction::Vertical)
                                        .constraints(
                                            [Constraint::Min(1), Constraint::Length(3)].as_ref(),
                                        )
                                        .split(area);
                                    let paragraph = Paragraph::new(lines.join("\n"));
                                    f.render_widget(paragraph, chunks[0]);
                                    let input_widget =
                                        Paragraph::new(format!("> {}", input));
                                    f.render_widget(input_widget, chunks[1]);
                                })?;
                            }
                        }
                        lines.push(current_line.clone());
                        if !current_line.is_empty() {
                            history.push(ChatMessage::assistant(current_line.clone()));
                        }
                        current_line.clear();

                        if tool_calls.is_empty() {
                            lines.push("─".repeat(80));
                        } else {
                            for call in tool_calls {
                                lines.push(format!(
                                    "🔧 [Calling tool: {} with args: {}]",
                                    call.function.name, call.function.arguments
                                ));
                                let result = call_mcp_tool(
                                    &call.function.name,
                                    call.function.arguments.clone(),
                                )
                                .await?;
                                lines.push(format!(
                                    "✅ [Tool {} completed: {}]",
                                    call.function.name, result
                                ));
                                history.push(ChatMessage::tool(
                                    result.clone(),
                                    call.function.name.clone(),
                                ));
                            }
                            lines.push("🤖 Assistant: ".to_string());
                            let mut final_line = String::new();
                            let request = ChatMessageRequest::new(
                                "gpt-oss:20b".to_string(),
                                history.clone(),
                            )
                            .tools(tool_infos.clone());
                            let mut stream =
                                ollama.send_chat_messages_stream(request).await?;
                            while let Some(chunk) = stream.next().await {
                                let chunk = match chunk {
                                    Ok(c) => c,
                                    Err(_) => break,
                                };
                                if !chunk.message.content.is_empty() {
                                    final_line.push_str(&chunk.message.content);
                                }
                                if chunk.done {
                                    break;
                                }
                                terminal.draw(|f| {
                                    let area = f.area();
                                    let chunks = Layout::default()
                                        .direction(Direction::Vertical)
                                        .constraints(
                                            [Constraint::Min(1), Constraint::Length(3)].as_ref(),
                                        )
                                        .split(area);
                                    let paragraph = Paragraph::new(lines.join("\n"));
                                    f.render_widget(paragraph, chunks[0]);
                                    let input_widget =
                                        Paragraph::new(format!("> {}", input));
                                    f.render_widget(input_widget, chunks[1]);
                                })?;
                            }
                            lines.push(final_line.clone());
                            if !final_line.is_empty() {
                                history
                                    .push(ChatMessage::assistant(final_line.clone()));
                            }
                            lines.push("─".repeat(80));
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
