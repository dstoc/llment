use std::{
    collections::HashMap,
    io::stdout,
    time::{Duration, Instant},
};

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
    text::Line,
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
    User(String),
    Assistant(String),
    Thinking {
        steps: Vec<ThinkingStep>,
        collapsed: bool,
        start: Instant,
        duration: Duration,
        done: bool,
    },
    Separator,
}

enum ThinkingStep {
    Thought(String),
    ToolCall {
        name: String,
        args: String,
        result: String,
        success: bool,
        collapsed: bool,
    },
}

enum LineMapping {
    Item(usize),
    Step { item: usize, step: usize },
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

fn wrap_history_lines(
    items: &[HistoryItem],
    width: usize,
) -> (Vec<String>, Vec<LineMapping>, Vec<bool>) {
    let mut lines = Vec::new();
    let mut mapping = Vec::new();
    let mut markdown = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        match item {
            HistoryItem::User(text) => {
                let inner_width = width.saturating_sub(7);
                let wrapped = wrap(text, inner_width.max(1));
                let box_width = wrapped.iter().map(|l| l.len()).max().unwrap_or(0);
                lines.push(format!("     ┌{}┐", "─".repeat(box_width)));
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                for w in wrapped {
                    let mut line = w.into_owned();
                    line.push_str(&" ".repeat(box_width.saturating_sub(line.len())));
                    lines.push(format!("     │{}│", line));
                    mapping.push(LineMapping::Item(idx));
                    markdown.push(false);
                }
                lines.push(format!("     └{}┘", "─".repeat(box_width)));
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
            }
            HistoryItem::Assistant(text) => {
                for w in wrap(text, width.max(1)) {
                    lines.push(w.into_owned());
                    mapping.push(LineMapping::Item(idx));
                    markdown.push(true);
                }
            }
            HistoryItem::Thinking {
                steps,
                collapsed,
                duration,
                done,
                ..
            } => {
                let calls = steps
                    .iter()
                    .filter(|s| matches!(s, ThinkingStep::ToolCall { .. }))
                    .count();
                if *done {
                    let summary = format!(
                        "Thought for {} seconds, {calls} tool call{}",
                        duration.as_secs(),
                        if calls == 1 { "" } else { "s" },
                    );
                    let arrow = if *collapsed { "›" } else { "⌄" };
                    lines.push(format!("{summary} {arrow}"));
                } else {
                    lines.push("Thinking ⌄".to_string());
                }
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
                if !*collapsed || !*done {
                    for (s_idx, step) in steps.iter().enumerate() {
                        match step {
                            ThinkingStep::Thought(t) => {
                                let wrapped = wrap(t, width.saturating_sub(2).max(1));
                                for (i, w) in wrapped.into_iter().enumerate() {
                                    if i == 0 {
                                        lines.push(format!("· {}", w));
                                    } else {
                                        lines.push(format!("  {}", w));
                                    }
                                    mapping.push(LineMapping::Step {
                                        item: idx,
                                        step: s_idx,
                                    });
                                    markdown.push(false);
                                }
                            }
                            ThinkingStep::ToolCall {
                                name,
                                args,
                                result,
                                success,
                                collapsed: tc_collapsed,
                            } => {
                                if *tc_collapsed {
                                    lines.push(format!("{name} ›"));
                                    mapping.push(LineMapping::Step {
                                        item: idx,
                                        step: s_idx,
                                    });
                                    markdown.push(false);
                                } else {
                                    lines.push(format!("{name} ⌄"));
                                    mapping.push(LineMapping::Step {
                                        item: idx,
                                        step: s_idx,
                                    });
                                    markdown.push(false);
                                    for w in wrap(
                                        &format!("args: {args}"),
                                        width.saturating_sub(2).max(1),
                                    ) {
                                        lines.push(format!("  {}", w));
                                        mapping.push(LineMapping::Step {
                                            item: idx,
                                            step: s_idx,
                                        });
                                        markdown.push(false);
                                    }
                                    let prefix = if *success { "result:" } else { "error:" };
                                    for w in wrap(
                                        &format!("{prefix} {result}"),
                                        width.saturating_sub(2).max(1),
                                    ) {
                                        lines.push(format!("  {}", w));
                                        mapping.push(LineMapping::Step {
                                            item: idx,
                                            step: s_idx,
                                        });
                                        markdown.push(false);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            HistoryItem::Separator => {
                lines.push("─".repeat(width));
                mapping.push(LineMapping::Item(idx));
                markdown.push(false);
            }
        }
    }
    (lines, mapping, markdown)
}

#[derive(Default)]
struct DrawState {
    history_rect: Rect,
    line_map: Vec<LineMapping>,
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
    let (lines, mapping, markdown_flags) = wrap_history_lines(items, width);
    let rendered_lines: Vec<Line> = lines
        .iter()
        .zip(markdown_flags.iter())
        .flat_map(|(line, &md)| {
            if md {
                from_str(line).lines
            } else {
                vec![Line::raw(line.clone())]
            }
        })
        .collect();
    let history_height = history_chunks[0].height as usize;
    let line_count = rendered_lines.len();
    let max_scroll = line_count.saturating_sub(history_height) as i32;
    *scroll_offset = (*scroll_offset).clamp(0, max_scroll);
    let top_line = (max_scroll - *scroll_offset) as usize;

    let paragraph = Paragraph::new(rendered_lines)
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
        line_map: mapping,
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
                        items.push(HistoryItem::User(query.clone()));
                        input.clear();
                        chat_history.push(ChatMessage::user(query.clone()));
                        let mut thinking_index: Option<usize> = None;
                        loop {
                            items.push(HistoryItem::Assistant(String::new()));
                            let mut assistant_index = items.len() - 1;
                            let mut current_line = String::new();
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
                                            t.push_str(thinking);
                                        } else {
                                            steps.push(ThinkingStep::Thought(thinking.clone()));
                                        }
                                    }
                                }
                                if !chunk.message.content.is_empty() {
                                    current_line.push_str(&chunk.message.content);
                                    if let HistoryItem::Assistant(line) =
                                        &mut items[assistant_index]
                                    {
                                        *line = current_line.clone();
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

                            if !current_line.is_empty() {
                                chat_history.push(ChatMessage::assistant(current_line.clone()));
                            } else {
                                items.remove(assistant_index);
                            }

                            if tool_calls.is_empty() {
                                if let Some(idx) = thinking_index {
                                    if let HistoryItem::Thinking {
                                        collapsed,
                                        start,
                                        duration,
                                        done,
                                        ..
                                    } = &mut items[idx]
                                    {
                                        *collapsed = true;
                                        *duration = start.elapsed();
                                        *done = true;
                                    }
                                }
                                items.push(HistoryItem::Separator);
                                break;
                            }

                            if let Some(t_idx) = thinking_index {
                                if let HistoryItem::Thinking { steps, .. } = &mut items[t_idx] {
                                    for call in tool_calls {
                                        steps.push(ThinkingStep::ToolCall {
                                            name: call.function.name.clone(),
                                            args: call.function.arguments.to_string(),
                                            result: String::new(),
                                            success: true,
                                            collapsed: true,
                                        });
                                        let s_idx = steps.len() - 1;
                                        let result = match call_mcp_tool(
                                            &call.function.name,
                                            call.function.arguments.clone(),
                                        )
                                        .await
                                        {
                                            Ok(res) => {
                                                if let ThinkingStep::ToolCall { result, .. } =
                                                    &mut steps[s_idx]
                                                {
                                                    *result = res.clone();
                                                }
                                                res
                                            }
                                            Err(err) => {
                                                if let ThinkingStep::ToolCall {
                                                    result,
                                                    success,
                                                    ..
                                                } = &mut steps[s_idx]
                                                {
                                                    *result = format!("Tool Failed: {}", err);
                                                    *success = false;
                                                }
                                                String::new()
                                            }
                                        };
                                        chat_history.push(ChatMessage::tool(
                                            result.clone(),
                                            call.function.name.clone(),
                                        ));
                                    }
                                }
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
                            if let Some(map) = draw_state.line_map.get(idx) {
                                match *map {
                                    LineMapping::Item(item_idx) => {
                                        if let Some(HistoryItem::Thinking {
                                            collapsed, done, ..
                                        }) = items.get_mut(item_idx)
                                        {
                                            if *done {
                                                *collapsed = !*collapsed;
                                            }
                                        }
                                    }
                                    LineMapping::Step { item, step } => {
                                        if let Some(HistoryItem::Thinking { steps, .. }) =
                                            items.get_mut(item)
                                        {
                                            if let Some(ThinkingStep::ToolCall {
                                                collapsed, ..
                                            }) = steps.get_mut(step)
                                            {
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

    Ok(())
}
