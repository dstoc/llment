use std::sync::{Arc, Mutex};

use crate::{
    Args, Component,
    components::{
        ErrorPopup, Prompt,
        completion::{Command, CommandInstance, Completion, CompletionResult},
        input::PromptModel,
    },
    conversation::{Conversation, ToolStep},
};
use clap::ValueEnum;
use crossterm::event::Event;
use futures_signals::signal::{Mutable, SignalExt};
use llm::{
    ChatMessage, ChatMessageRequest, LlmClient, MessageRole, Provider,
    mcp::{McpContext, McpToolExecutor},
    tools::{ToolEvent, ToolExecutor, tool_event_stream},
};
use ratatui::{prelude::*, widgets::Paragraph};
use tokio::{
    sync::{
        OnceCell,
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        oneshot,
    },
    task::JoinSet,
};
use tokio_stream::StreamExt;
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

    client: Arc<Mutex<llm::Client>>,
    tool_executor: Arc<dyn ToolExecutor>,
    mcp_context: Arc<McpContext>,
    session_in_tokens: u32,
    session_out_tokens: u32,
    chat_history: Vec<ChatMessage>,
    state: ConversationState,

    tasks: JoinSet<()>,
    request_tasks: JoinSet<()>,
    update_tx: UnboundedSender<Update>,
    update_rx: UnboundedReceiver<Update>,
    ignore_responses: bool,
    error: ErrorPopup,
}

pub struct AppModel {
    pub needs_update: Mutable<bool>,
    pub needs_redraw: Mutable<bool>,
    pub should_quit: Mutable<bool>,
}

enum Update {
    Prompt(String),
    Response(ToolEvent),
    History(Vec<ChatMessage>),
    Error(String),
    SetModel(String),
    SetProvider(Provider, Option<String>),
    Redo,
    Clear,
}

impl App {
    // TODO: mcp_context should not be a param
    pub fn new(model: AppModel, args: Args, mcp_context: McpContext) -> Self {
        let (update_tx, update_rx) = unbounded_channel();
        let mcp_context = Arc::new(mcp_context);
        let tool_executor: Arc<dyn ToolExecutor> =
            Arc::new(McpToolExecutor::new(mcp_context.clone()));
        let client = llm::client_from(args.provider, args.model.clone(), Some(&args.host)).unwrap();
        let client = Arc::new(Mutex::new(client));
        let tasks = JoinSet::new();
        let request_tasks = JoinSet::new();
        let needs_redraw = model.needs_redraw.clone();
        App {
            conversation: Conversation::default(),
            prompt: Prompt::new(
                PromptModel {
                    needs_redraw: model.needs_redraw.clone(),
                    needs_update: model.needs_update.clone(),
                    ..Default::default()
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
                    Box::new(RedoCommand {
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
            client,
            session_in_tokens: 0,
            session_out_tokens: 0,
            tool_executor,
            mcp_context,
            chat_history: vec![],
            state: ConversationState::Idle,
            tasks,
            request_tasks,
            update_tx,
            update_rx,
            ignore_responses: false,
            error: ErrorPopup::new(needs_redraw),
        }
    }

    fn handle_tool_event(&mut self, ev: ToolEvent) {
        match ev {
            ToolEvent::Chunk(chunk) => {
                if let Some(thinking) = chunk.message.thinking.as_ref() {
                    self.state = ConversationState::Thinking;
                    self.model.needs_redraw.set(true);
                    self.conversation.append_thinking(thinking);
                }
                if let Some(content) = chunk.message.content.as_ref() {
                    if !content.is_empty() {
                        self.state = ConversationState::Responding;
                        self.model.needs_redraw.set(true);
                        self.conversation.append_response(content);
                    }
                }
                if chunk.done {
                    if let Some(usage) = chunk.usage {
                        self.session_in_tokens += usage.input_tokens;
                        self.session_out_tokens += usage.output_tokens;
                        self.conversation.set_usage(usage);
                    }
                }
            }
            ToolEvent::ToolStarted { id, name, args } => {
                self.state = ConversationState::CallingTool(name.clone());
                self.model.needs_redraw.set(true);
                self.conversation.add_tool_step(ToolStep::new(
                    name,
                    id,
                    args.to_string(),
                    String::new(),
                    true,
                ));
            }
            ToolEvent::ToolResult { id, result, .. } => {
                let (text, failed) = match result {
                    Ok(t) => (t, false),
                    Err(e) => (format!("Tool Failed: {}", e), true),
                };
                self.conversation.update_tool_result(id, text, failed);
            }
        }
    }

    fn send_request(&mut self, prompt: String) -> () {
        self.state = ConversationState::Thinking;
        self.model.needs_redraw.set(true);
        self.conversation.push_user(prompt.clone());
        self.chat_history.push(ChatMessage::user(prompt));
        self.conversation.push_assistant_block();
        let tool_infos = self.mcp_context.tool_infos.clone();
        let model_name = { self.client.lock().unwrap().model().to_string() };
        let request = ChatMessageRequest::new(model_name, self.chat_history.clone())
            .tools(tool_infos)
            .think(true);
        let history = std::mem::take(&mut self.chat_history);
        let client = { Arc::new(self.client.lock().unwrap().clone()) };
        let (mut stream, handle) =
            tool_event_stream(client, request, self.tool_executor.clone(), history);

        self.ignore_responses = false;
        let update_tx = self.update_tx.clone();
        let needs_update = self.model.needs_update.clone();
        self.request_tasks.spawn(async move {
            while let Some(event) = stream.next().await {
                let _ = update_tx.send(Update::Response(event));
                needs_update.set(true);
            }
            match handle.await {
                Ok(Ok(history)) => {
                    let _ = update_tx.send(Update::History(history));
                }
                Ok(Err(err)) => {
                    let _ = update_tx.send(Update::Error(err.to_string()));
                }
                Err(err) => {
                    let _ = update_tx.send(Update::Error(err.to_string()));
                }
            }
            needs_update.set(true);
        });
    }

    fn abort_requests(&mut self) {
        self.request_tasks.abort_all();
        self.request_tasks = JoinSet::new();
        self.ignore_responses = true;
    }
}

impl Component for App {
    fn init(&mut self) {
        let needs_update = self.model.needs_update.clone();
        let update_tx = self.update_tx.clone();
        let mut new_prompts = self
            .prompt
            .model
            .submitted_prompt
            .signal_cloned()
            .to_stream();
        self.tasks.spawn(async move {
            loop {
                if let Some(prompt) = new_prompts.next().await {
                    let _ = update_tx.send(Update::Prompt(prompt));
                    needs_update.set(true);
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
                self.model.needs_redraw.set(true);
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
                        self.send_request(prompt);
                    }
                }
                Ok(Update::Response(event)) => {
                    if !self.ignore_responses {
                        self.handle_tool_event(event);
                        // TODO: conversation should do this
                        self.model.needs_redraw.set(true);
                    }
                }
                Ok(Update::History(history)) => {
                    if !self.ignore_responses {
                        self.chat_history = history;
                    }
                    self.state = ConversationState::Idle;
                    self.model.needs_redraw.set(true);
                }
                Ok(Update::Error(err)) => {
                    self.error.set(err);
                    self.state = ConversationState::Idle;
                    self.model.needs_redraw.set(true);
                }
                Ok(Update::SetModel(model_name)) => {
                    self.abort_requests();
                    {
                        let mut client = self.client.lock().unwrap();
                        client.set_model(model_name);
                    }
                    self.model.needs_redraw.set(true);
                }
                Ok(Update::SetProvider(provider, host)) => {
                    self.abort_requests();
                    let model = { self.client.lock().unwrap().model().to_string() };
                    if let Ok(new_client) = llm::client_from(provider, model, host.as_deref()) {
                        {
                            let mut guard = self.client.lock().unwrap();
                            *guard = new_client;
                        }
                        self.model.needs_redraw.set(true);
                    }
                }
                Ok(Update::Clear) => {
                    self.abort_requests();
                    self.chat_history.clear();
                    self.conversation.clear();
                    self.session_in_tokens = 0;
                    self.session_out_tokens = 0;
                    self.state = ConversationState::Idle;
                    self.model.needs_redraw.set(true);
                }
                Ok(Update::Redo) => {
                    if let Some(text) = self.conversation.redo_last() {
                        self.abort_requests();
                        while let Some(msg) = self.chat_history.pop() {
                            if msg.role == MessageRole::User {
                                break;
                            }
                        }
                        self.prompt.set_prompt(text);
                    }
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
            .margin(1)
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
        let ctx_tokens = self.conversation.context_tokens();
        let status_right = format!(
            "ctx {}t, Σ {}t=>{}t",
            ctx_tokens, self.session_in_tokens, self.session_out_tokens
        );
        let right_width = status_right.width() as u16;
        let status_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(right_width)].as_ref())
            .split(chunks[3]);
        let state_text = match &self.state {
            ConversationState::Idle => String::new(),
            ConversationState::Thinking => "thinking…".to_string(),
            ConversationState::CallingTool(name) => format!("tool: {}", name),
            ConversationState::Responding => "responding…".to_string(),
        };
        let status_left = {
            let client = self.client.lock().unwrap();
            format!("{} {:?} {}", state_text, client.provider(), client.model())
        };
        frame.render_widget(Paragraph::new(status_left), status_chunks[0]);
        frame.render_widget(
            Paragraph::new(status_right).alignment(Alignment::Right),
            status_chunks[1],
        );
    }
}

struct ModelCommand {
    client: Arc<Mutex<llm::Client>>,
    tx: UnboundedSender<Update>,
}

impl Command for ModelCommand {
    fn name(&self) -> &'static str {
        "model"
    }
    fn description(&self) -> &'static str {
        "Change the active model"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(ModelCommandInstance {
            tx: self.tx.clone(),
            client: self.client.clone(),
            models: Arc::default(),
            param: String::default(),
        })
    }
}

struct ModelCommandInstance {
    tx: UnboundedSender<Update>,
    client: Arc<Mutex<llm::Client>>,
    models: Arc<OnceCell<Vec<String>>>,
    param: String,
}
impl ModelCommandInstance {
    fn matching(&self) -> Vec<Completion> {
        if let Some(models) = self.models.get() {
            let param = self.param.as_str();
            models
                .iter()
                .filter(|model| model.starts_with(param))
                .map(|model| Completion {
                    name: model.clone(),
                    description: "".to_string(),
                    str: model.clone(),
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}
impl CommandInstance for ModelCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        let param = input.trim();
        self.param = param.to_string();
        if let Some(_) = self.models.get() {
            let options = self.matching();
            // TODO: if we don't match any, then it could be an error?
            CompletionResult::Options { at: 0, options }
        } else {
            let client_handle = self.client.clone();
            let models = self.models.clone();
            let (tx, rx) = oneshot::channel();
            tokio::spawn(async move {
                let client = { client_handle.lock().unwrap().clone() };
                let _ = models
                    .get_or_init(|| async move {
                        match client.list_models().await {
                            Ok(models) => models,
                            Err(_) => Vec::new(), // TODO: surface an error?
                        }
                    })
                    .await;

                let _ = tx.send(());
            });
            CompletionResult::Loading { at: 0, done: rx }
        }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.param.is_empty() {
            Err("no param".into())
        } else {
            println!("commit model??");
            let _ = self.tx.send(Update::SetModel(self.param.clone()));
            Ok(())
        }
    }
}

struct ProviderCommand {
    needs_update: Mutable<bool>,
    update_tx: UnboundedSender<Update>,
}

impl Command for ProviderCommand {
    fn name(&self) -> &'static str {
        "provider"
    }
    fn description(&self) -> &'static str {
        "Change the active provider"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(ProviderCommandInstance {
            needs_update: self.needs_update.clone(),
            tx: self.update_tx.clone(),
            param: String::new(),
        })
    }
}

struct ProviderCommandInstance {
    needs_update: Mutable<bool>,
    tx: UnboundedSender<Update>,
    param: String,
}

impl ProviderCommandInstance {
    fn provider_options(&self, typed: &str) -> Vec<Completion> {
        Provider::value_variants()
            .iter()
            .filter_map(|p| {
                let name = p.to_possible_value()?.get_name().to_string();
                if name.starts_with(typed) {
                    Some(Completion {
                        name: name.clone(),
                        description: String::new(),
                        str: format!("{} ", name),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

impl CommandInstance for ProviderCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        self.param = input.trim().to_string();
        let (prov, host_opt) = match self.param.split_once(' ') {
            Some((p, h)) => (p, Some(h)),
            None => (self.param.as_str(), None),
        };
        if host_opt.is_none() {
            let options = self.provider_options(prov);
            CompletionResult::Options { at: 0, options }
        } else {
            CompletionResult::Options {
                at: 0,
                options: vec![],
            }
        }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.param.is_empty() {
            return Err("no provider".into());
        }
        let mut parts = self.param.split_whitespace();
        let prov_str = parts.next().ok_or("no provider")?;
        let provider = Provider::from_str(prov_str, true)?;
        let host = parts.next().map(|s| s.to_string());
        let _ = self.tx.send(Update::SetProvider(provider, host));
        self.needs_update.set(true);
        Ok(())
    }
}

struct QuitCommand {
    should_quit: Mutable<bool>,
}

impl Command for QuitCommand {
    fn name(&self) -> &'static str {
        "quit"
    }
    fn description(&self) -> &'static str {
        "Exit the application"
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(QuitCommandInstance {
            should_quit: self.should_quit.clone(),
        })
    }
}

struct QuitCommandInstance {
    should_quit: Mutable<bool>,
}

impl CommandInstance for QuitCommandInstance {
    fn update(&mut self, _input: &str) -> CompletionResult {
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.should_quit.set(true);
        Ok(())
    }
}

struct RedoCommand {
    needs_update: Mutable<bool>,
    update_tx: UnboundedSender<Update>,
}

impl Command for RedoCommand {
    fn name(&self) -> &'static str {
        "redo"
    }
    fn description(&self) -> &'static str {
        "Rewrite the last prompt"
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(RedoCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
        })
    }
}

struct RedoCommandInstance {
    needs_update: Mutable<bool>,
    update_tx: UnboundedSender<Update>,
}

impl CommandInstance for RedoCommandInstance {
    fn update(&mut self, _input: &str) -> CompletionResult {
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.update_tx.send(Update::Redo);
        self.needs_update.set(true);
        Ok(())
    }
}

struct ClearCommand {
    needs_update: Mutable<bool>,
    update_tx: UnboundedSender<Update>,
}

impl Command for ClearCommand {
    fn name(&self) -> &'static str {
        "clear"
    }
    fn description(&self) -> &'static str {
        "Clear the conversation history"
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(ClearCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
        })
    }
}

struct ClearCommandInstance {
    needs_update: Mutable<bool>,
    update_tx: UnboundedSender<Update>,
}

impl CommandInstance for ClearCommandInstance {
    fn update(&mut self, _input: &str) -> CompletionResult {
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.update_tx.send(Update::Clear);
        self.needs_update.set(true);
        Ok(())
    }
}
