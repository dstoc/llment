use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    Args, Component,
    builtins::setup_builtin_tools,
    commands::{
        AgentModeCommand, ClearCommand, ContinueCommand, LoadCommand, ModelCommand, PopCommand,
        PromptCommand, ProviderCommand, QuitCommand, RedoCommand, ResponseCommand, RoleCommand,
        SaveCommand, ThoughtCommand,
    },
    components::{ErrorPopup, Prompt, input::PromptModel},
    conversation::{Conversation, ToolStep},
    history_edits::{HistoryEdit, HistoryEditResult},
    modes::AgentMode,
    prompts,
};
use crossterm::event::Event;
use llm::{
    AssistantPart, ChatMessage, ChatMessageRequest, JsonResult, Provider, ResponseChunk,
    mcp::{McpContext, McpService},
    tools::{ToolEvent, ToolExecutor, tool_event_stream},
};
use ratatui::{prelude::*, widgets::Paragraph};
use rmcp::service::{RoleClient, RunningService};
use tokio::{
    sync::{
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        watch,
    },
    task::JoinSet,
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tui_realm_stdlib::states::SpinnerStates;
use unicode_width::UnicodeWidthStr;

enum ConversationState {
    Idle,
    Thinking,
    CallingTool(String),
    Responding,
}

pub struct App {
    pub model: AppModel,
    conversation: Conversation,
    prompt: Prompt,

    prompt_dir: Option<PathBuf>,

    client: Arc<Mutex<llm::Client>>,
    mcp_context: McpContext,
    request_in_tokens: u32,
    request_out_tokens: u32,
    session_in_tokens: u32,
    session_out_tokens: u32,
    session_requests: u32,
    chat_history: Arc<Mutex<Vec<ChatMessage>>>,
    state: ConversationState,
    spinner: SpinnerStates,

    tasks: JoinSet<()>,
    request_tasks: JoinSet<()>,
    update_tx: UnboundedSender<Update>,
    update_rx: UnboundedReceiver<Update>,
    ignore_responses: bool,
    error: ErrorPopup,
    selected_prompt: Option<String>,
    selected_role: Option<String>,
    mode: Option<Box<dyn AgentMode>>,
}

pub struct AppModel {
    pub needs_update: watch::Sender<bool>,
    pub needs_redraw: watch::Sender<bool>,
    pub should_quit: watch::Sender<bool>,
}

pub(crate) enum Update {
    Prompt(String),
    Response(ToolEvent),
    ResponseComplete,
    Error(String),
    SetModel(String),
    SetProvider(Provider, Option<String>),
    SetPrompt(String),
    SetRole(Option<String>),
    Continue,
    EditHistory(HistoryEdit),
    SetMode(
        Option<Box<dyn AgentMode>>,
        Option<RunningService<RoleClient, McpService>>,
    ),
}

impl App {
    pub fn new(model: AppModel, args: Args) -> Self {
        let (update_tx, update_rx) = unbounded_channel();
        let mcp_context = McpContext::default();
        let prompt_dir = args.prompt_dir.clone();
        let client =
            llm::client_from(args.provider, args.model.clone(), args.host.as_deref()).unwrap();
        let client = Arc::new(Mutex::new(client));
        let tasks = JoinSet::new();
        let request_tasks = JoinSet::new();
        let mut spinner = SpinnerStates::default();
        spinner.reset("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏");
        let needs_redraw = model.needs_redraw.clone();
        App {
            conversation: Conversation::default(),
            prompt: Prompt::new(
                PromptModel {
                    needs_redraw: model.needs_redraw.clone(),
                    needs_update: model.needs_update.clone(),
                },
                vec![
                    Box::new(ModelCommand {
                        client: client.clone(),
                        tx: update_tx.clone(),
                    }),
                    Box::new(ProviderCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(PromptCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                        prompt_dir: prompt_dir.clone(),
                    }),
                    Box::new(RoleCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                        prompt_dir: prompt_dir.clone(),
                    }),
                    Box::new(AgentModeCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(RedoCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(PopCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(ContinueCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(ThoughtCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(ResponseCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(SaveCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(LoadCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(ClearCommand {
                        needs_update: model.needs_update.clone(),
                        update_tx: update_tx.clone(),
                    }),
                    Box::new(QuitCommand {
                        should_quit: model.should_quit.clone(),
                    }),
                ],
            ),
            model,
            prompt_dir,
            client,
            session_in_tokens: 0,
            session_out_tokens: 0,
            session_requests: 0,
            request_in_tokens: 0,
            request_out_tokens: 0,
            mcp_context,
            chat_history: Arc::new(Mutex::new(vec![])),
            state: ConversationState::Idle,
            spinner: spinner,
            tasks,
            request_tasks,
            update_tx,
            update_rx,
            ignore_responses: false,
            error: ErrorPopup::new(needs_redraw),
            selected_prompt: Some("default".to_string()),
            selected_role: None,
            mode: None,
        }
    }

    pub async fn init(&mut self, mcp_context: McpContext) {
        self.mcp_context = mcp_context;
        let builtin_service = setup_builtin_tools(self.chat_history.clone()).await;
        self.mcp_context
            .insert(builtin_service)
            .expect("builtin MCP prefix must not contain '_'");
    }

    fn handle_tool_event(&mut self, ev: ToolEvent) {
        match ev {
            ToolEvent::RequestStarted => {
                self.request_in_tokens = 0;
                self.request_out_tokens = 0;
                self.session_requests += 1;
                let _ = self.model.needs_redraw.send(true);
            }
            ToolEvent::Chunk(chunk) => match chunk {
                ResponseChunk::Part(AssistantPart::Thinking { text, .. }) => {
                    self.state = ConversationState::Thinking;
                    let _ = self.model.needs_redraw.send(true);
                    self.conversation.append_thinking(&text);
                }
                ResponseChunk::Part(AssistantPart::Text { text, .. }) => {
                    if !text.is_empty() {
                        self.state = ConversationState::Responding;
                        let _ = self.model.needs_redraw.send(true);
                        self.conversation.append_response(&text);
                    }
                }
                ResponseChunk::Part(AssistantPart::ToolCall { .. }) => {}
                ResponseChunk::Usage {
                    input_tokens,
                    output_tokens,
                } => {
                    self.session_in_tokens += input_tokens;
                    self.session_out_tokens += output_tokens;
                    self.request_in_tokens += input_tokens;
                    self.request_out_tokens += output_tokens;
                    let _ = self.model.needs_redraw.send(true);
                }
                ResponseChunk::Done => {}
            },
            ToolEvent::ToolStarted {
                call_id,
                name,
                args,
            } => {
                self.state = ConversationState::CallingTool(name.clone());
                let _ = self.model.needs_redraw.send(true);
                let arg_str = match args {
                    JsonResult::Content { content } => content.to_string(),
                    JsonResult::Error { error } => error,
                };
                self.conversation.add_tool_step(ToolStep::new(
                    name,
                    call_id,
                    arg_str,
                    String::new(),
                    true,
                ));
            }
            ToolEvent::ToolResult {
                call_id, result, ..
            } => {
                let (text, failed) = match result {
                    Ok(t) => (t, false),
                    Err(e) => (format!("Tool Failed: {}", e), true),
                };
                self.conversation.update_tool_result(&call_id, text, failed);
            }
        }
    }

    fn apply_prompt(&mut self) {
        if let Some(name) = &self.selected_prompt {
            let tool_names = self.mcp_context.tool_names();
            let role = self.selected_role.as_deref();
            if let Some(content) =
                prompts::load_prompt(name, role, tool_names, self.prompt_dir.as_deref())
            {
                let mut history = self.chat_history.lock().unwrap();
                while matches!(history.first(), Some(ChatMessage::System(_))) {
                    history.remove(0);
                }
                history.insert(0, ChatMessage::system(content));
            }
        }
    }

    fn send_request(&mut self, prompt: Option<String>) {
        self.apply_prompt();
        self.state = ConversationState::Thinking;
        let _ = self.model.needs_redraw.send(true);
        if let Some(prompt) = prompt {
            self.conversation.push_user(prompt.clone());
            {
                let mut history = self.chat_history.lock().unwrap();
                history.push(ChatMessage::user(prompt));
            }
        }

        self.ignore_responses = false;
        let update_tx = self.update_tx.clone();
        let needs_update = self.model.needs_update.clone();
        let history = self.chat_history.clone();
        let mcp_context = Arc::new(self.mcp_context.clone());
        let client = { Arc::new(self.client.lock().unwrap().clone()) };
        self.request_tasks.spawn(async move {
            let tool_infos = mcp_context.tool_infos();
            let model_name = { client.model().to_string() };
            let request_history = { history.lock().unwrap().clone() };
            let request = ChatMessageRequest::new(model_name, request_history)
                .tools(tool_infos)
                .think(true);
            let tool_executor = mcp_context.clone() as Arc<dyn ToolExecutor>;
            let (mut stream, handle) =
                tool_event_stream(client, request, tool_executor, history.clone());
            while let Some(event) = stream.next().await {
                let _ = update_tx.send(Update::Response(event));
                let _ = needs_update.send(true);
            }
            match handle.await {
                Ok(Ok(())) => {
                    let _ = update_tx.send(Update::ResponseComplete);
                }
                Ok(Err(err)) => {
                    let _ = update_tx.send(Update::Error(err.to_string()));
                }
                Err(err) => {
                    let _ = update_tx.send(Update::Error(err.to_string()));
                }
            }
            let _ = needs_update.send(true);
        });
    }

    fn abort_requests(&mut self) {
        self.request_tasks.abort_all();
        self.request_tasks = JoinSet::new();
        self.ignore_responses = true;
        self.state = ConversationState::Idle;
    }

    fn clear(&mut self) {
        self.abort_requests();
        self.chat_history.lock().unwrap().clear();
        self.conversation.clear();
        self.state = ConversationState::Idle;
    }
}

impl Component for App {
    fn init(&mut self) {
        let needs_update = self.model.needs_update.clone();
        let update_tx = self.update_tx.clone();
        let mut new_prompts = WatchStream::new(self.prompt.submitted_prompt_rx());
        self.tasks.spawn(async move {
            loop {
                if let Some(prompt) = new_prompts.next().await {
                    let _ = update_tx.send(Update::Prompt(prompt));
                    let _ = needs_update.send(true);
                } else {
                    break;
                }
            }
        });
    }
    fn handle_event(&mut self, event: Event) {
        self.error.handle_event(event.clone());
        match event {
            Event::Key(key) => {
                self.prompt.handle_event(Event::Key(key));
            }
            Event::Mouse(_) => {
                self.conversation.handle_event(event);
                // TODO: conversation should do this
                let _ = self.model.needs_redraw.send(true);
            }
            Event::Paste(_) => {
                self.prompt.handle_event(event);
            }
            _ => (),
        }
    }

    fn update(&mut self) {
        self.conversation.update();
        self.prompt.update();
        self.error.update();

        loop {
            match self.update_rx.try_recv() {
                Ok(Update::Prompt(prompt)) => {
                    if !prompt.is_empty() {
                        self.send_request(Some(prompt));
                    }
                }
                Ok(Update::Continue) => {
                    self.send_request(None);
                }
                Ok(Update::Response(event)) => {
                    if !self.ignore_responses {
                        self.handle_tool_event(event);
                        // TODO: conversation should do this
                        let _ = self.model.needs_redraw.send(true);
                    }
                }
                Ok(Update::ResponseComplete) => {
                    self.state = ConversationState::Idle;
                    let last_message = { self.chat_history.lock().unwrap().last().cloned() };
                    let step = if let Some(mode) = self.mode.as_mut() {
                        Some(mode.step(last_message.as_ref()))
                    } else {
                        None
                    };
                    if let Some(step) = step {
                        if step.clear_history {
                            self.clear();
                        }
                        self.selected_role = step.role;
                        if step.stop {
                            self.mcp_context.remove("agent");
                            self.mode = None;
                        } else {
                            self.send_request(step.prompt);
                        }
                    }
                    let _ = self.model.needs_redraw.send(true);
                }
                Ok(Update::Error(err)) => {
                    self.error.set(err);
                    self.state = ConversationState::Idle;
                    let _ = self.model.needs_redraw.send(true);
                }
                Ok(Update::SetModel(model_name)) => {
                    self.abort_requests();
                    {
                        let mut client = self.client.lock().unwrap();
                        client.set_model(model_name);
                    }
                    let _ = self.model.needs_redraw.send(true);
                }
                Ok(Update::SetProvider(provider, host)) => {
                    self.abort_requests();
                    let model = { self.client.lock().unwrap().model().to_string() };
                    if let Ok(new_client) = llm::client_from(provider, model, host.as_deref()) {
                        {
                            let mut guard = self.client.lock().unwrap();
                            *guard = new_client;
                        }
                        let _ = self.model.needs_redraw.send(true);
                    }
                }
                Ok(Update::SetPrompt(name)) => {
                    self.selected_prompt = Some(name);
                }
                Ok(Update::SetRole(role)) => {
                    self.selected_role = role;
                }
                Ok(Update::EditHistory(edit)) => {
                    let history_arc = self.chat_history.clone();
                    let mut history_guard = history_arc.lock().unwrap();
                    let result = edit(&mut history_guard);
                    let history = history_guard.clone();
                    let HistoryEditResult {
                        prompt,
                        reset_session,
                        abort_requests,
                    } = match result {
                        Ok(res) => res,
                        Err(err) => {
                            drop(history_guard);
                            self.error.set(err);
                            let _ = self.model.needs_redraw.send(true);
                            continue;
                        }
                    };
                    if abort_requests {
                        self.abort_requests();
                    }
                    drop(history_guard);
                    if reset_session {
                        self.session_in_tokens = 0;
                        self.session_out_tokens = 0;
                        self.session_requests = 0;
                    }
                    if let Some(p) = prompt {
                        self.prompt.set_prompt(p);
                    }
                    self.conversation.set_history(&history);
                    let _ = self.model.needs_redraw.send(true);
                }
                Ok(Update::SetMode(mode, service)) => {
                    self.mcp_context.remove("agent");
                    self.abort_requests();
                    self.mode = mode;
                    if let Some(service) = service {
                        if let Err(err) = self.mcp_context.insert(service) {
                            self.mode = None;
                            self.error.set(err.to_string());
                            let _ = self.model.needs_redraw.send(true);
                            continue;
                        }
                    }
                    let start = if let Some(mode) = self.mode.as_mut() {
                        Some(mode.start())
                    } else {
                        None
                    };
                    if let Some(start) = start {
                        if start.clear_history {
                            self.clear();
                        }
                        self.selected_role = start.role;
                        self.send_request(start.prompt);
                    } else {
                        self.selected_role = None;
                    }
                    let _ = self.model.needs_redraw.send(true);
                }
                Err(_) => break,
            }
        }
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        let prompt_height = self.prompt.height();
        let inner_width = area.width.saturating_sub(2);
        let error_height = self.error.height(inner_width);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Min(1),
                    Constraint::Length(error_height),
                    Constraint::Length(prompt_height),
                    Constraint::Length(1),
                ]
                .as_ref(),
            )
            .split(area);

        self.conversation.render(frame, chunks[0]);
        self.error.render(frame, chunks[1]);
        self.prompt.render(frame, chunks[2]);
        let ctx_tokens = self.request_in_tokens + self.request_out_tokens;
        let status_right = format!(
            "ctx {}t, Σ {}r {}t=>{}t",
            ctx_tokens, self.session_requests, self.session_in_tokens, self.session_out_tokens
        );
        let right_width = status_right.width() as u16;
        let status_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(right_width)].as_ref())
            .split(chunks[3]);
        let state_text = match &self.state {
            ConversationState::Idle => String::new(),
            ConversationState::Thinking => format!("thinking… {}", self.spinner.step()),
            ConversationState::CallingTool(name) => format!("tool: {}", name),
            ConversationState::Responding => format!("responding… {}", self.spinner.step()),
        };
        let status_left = {
            let client = self.client.lock().unwrap();
            let mut parts = vec![
                format!("{:?}", client.provider()),
                client.model().to_string(),
            ];
            if let Some(prompt) = &self.selected_prompt {
                if prompt != "default" {
                    parts.push(prompt.clone());
                }
            }
            if let Some(role) = &self.selected_role {
                parts.push(role.clone());
            }
            if !state_text.is_empty() {
                parts.push(state_text);
            }
            parts.join(" ")
        };
        frame.render_widget(Paragraph::new(status_left), status_chunks[0]);
        frame.render_widget(
            Paragraph::new(status_right).alignment(Alignment::Right),
            status_chunks[1],
        );
    }
}
