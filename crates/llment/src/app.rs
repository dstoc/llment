use std::sync::{Arc, Mutex};

use crate::{
    Args, Component,
    builtins::setup_builtin_tools,
    components::{
        ErrorPopup, Prompt,
        completion::{Command, CommandInstance, Completion, CompletionResult},
        input::PromptModel,
    },
    conversation::{Conversation, ToolStep},
};
use clap::ValueEnum;
use crossterm::event::Event;
use globset::Glob;
use llm::{
    ChatMessage, ChatMessageRequest, LlmClient, Provider,
    mcp::McpContext,
    tools::{ToolEvent, ToolExecutor, tool_event_stream},
};
use minijinja::Environment;
use ratatui::{prelude::*, widgets::Paragraph};
use rust_embed::RustEmbed;
use tokio::{
    sync::{
        OnceCell,
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        oneshot, watch,
    },
    task::JoinSet,
};
use tokio_stream::{StreamExt, wrappers::WatchStream};
use tui_realm_stdlib::states::SpinnerStates;
use unicode_width::UnicodeWidthStr;

#[derive(RustEmbed)]
#[folder = "prompts"]
struct PromptAssets;

fn load_prompt(name: &str) -> Option<String> {
    let mut env = Environment::new();
    env.set_loader(|name| {
        let mut candidates: Vec<String> = vec![name.to_string()];
        if !name.ends_with(".md.jinja") {
            candidates.push(format!("{}.md.jinja", name));
        }
        if !name.ends_with(".md") {
            candidates.push(format!("{}.md", name));
        }
        for candidate in candidates {
            if let Some(file) = PromptAssets::get(&candidate) {
                let content = String::from_utf8_lossy(file.data.as_ref()).to_string();
                return Ok(Some(content));
            }
        }
        Ok(None)
    });
    env.add_function(
        "glob",
        |pattern: String| -> Result<Vec<String>, minijinja::Error> {
            let glob = Glob::new(&pattern).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
            })?;
            let matcher = glob.compile_matcher();
            let mut matches: Vec<String> = PromptAssets::iter()
                .map(|f| f.as_ref().to_string())
                .filter(|name| matcher.is_match(name))
                .collect();
            matches.sort();
            Ok(matches)
        },
    );
    let jinja_name = format!("{}.md.jinja", name);
    if let Ok(tmpl) = env.get_template(&jinja_name) {
        tmpl.render(()).ok()
    } else if let Some(file) = PromptAssets::get(&format!("{}.md", name)) {
        Some(String::from_utf8_lossy(file.data.as_ref()).to_string())
    } else {
        None
    }
}

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
    mcp_context: Arc<McpContext>,
    session_in_tokens: u32,
    session_out_tokens: u32,
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
}

pub struct AppModel {
    pub needs_update: watch::Sender<bool>,
    pub needs_redraw: watch::Sender<bool>,
    pub should_quit: watch::Sender<bool>,
}

enum Update {
    Prompt(String),
    Response(ToolEvent),
    History(Vec<ChatMessage>),
    Error(String),
    SetModel(String),
    SetProvider(Provider, Option<String>),
    SetPrompt(String),
    Redo,
    Clear,
}

impl App {
    pub fn new(model: AppModel, args: Args) -> Self {
        let (update_tx, update_rx) = unbounded_channel();
        let mcp_context = Arc::new(McpContext::default());
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
            selected_prompt: None,
        }
    }

    pub async fn init(&mut self, mut mcp_context: McpContext) {
        let builtin_service = setup_builtin_tools(self.chat_history.clone()).await;
        mcp_context.insert(builtin_service);
        self.mcp_context = Arc::new(mcp_context);
    }

    fn handle_tool_event(&mut self, ev: ToolEvent) {
        match ev {
            ToolEvent::Chunk(chunk) => {
                if let Some(thinking) = chunk.message.thinking.as_ref() {
                    self.state = ConversationState::Thinking;
                    let _ = self.model.needs_redraw.send(true);
                    self.conversation.append_thinking(thinking);
                }
                if let Some(content) = chunk.message.content.as_ref() {
                    if !content.is_empty() {
                        self.state = ConversationState::Responding;
                        let _ = self.model.needs_redraw.send(true);
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
                let _ = self.model.needs_redraw.send(true);
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

    fn apply_prompt(&mut self) {
        if let Some(name) = &self.selected_prompt {
            if let Some(content) = load_prompt(name) {
                let mut history = self.chat_history.lock().unwrap();
                while matches!(history.first(), Some(ChatMessage::System(_))) {
                    history.remove(0);
                }
                history.insert(0, ChatMessage::system(content));
            }
        }
    }

    fn send_request(&mut self, prompt: String) -> () {
        self.state = ConversationState::Thinking;
        let _ = self.model.needs_redraw.send(true);
        self.conversation.push_user(prompt.clone());
        self.conversation.push_assistant_block();
        {
            let mut history = self.chat_history.lock().unwrap();
            history.push(ChatMessage::user(prompt));
        }

        self.ignore_responses = false;
        let update_tx = self.update_tx.clone();
        let needs_update = self.model.needs_update.clone();
        let history = self.chat_history.clone();
        let mcp_context = self.mcp_context.clone();
        let client = { Arc::new(self.client.lock().unwrap().clone()) };
        self.request_tasks.spawn(async move {
            let tool_infos = mcp_context.tool_infos().await;
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
                    let history = { history.lock().unwrap().clone() };
                    let _ = update_tx.send(Update::History(history));
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
                        self.send_request(prompt);
                    }
                }
                Ok(Update::Response(event)) => {
                    if !self.ignore_responses {
                        self.handle_tool_event(event);
                        // TODO: conversation should do this
                        let _ = self.model.needs_redraw.send(true);
                    }
                }
                Ok(Update::History(history)) => {
                    if !self.ignore_responses {
                        *self.chat_history.lock().unwrap() = history;
                    }
                    self.state = ConversationState::Idle;
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
                    self.apply_prompt();
                }
                Ok(Update::Clear) => {
                    self.abort_requests();
                    self.chat_history.lock().unwrap().clear();
                    self.conversation.clear();
                    self.session_in_tokens = 0;
                    self.session_out_tokens = 0;
                    self.state = ConversationState::Idle;
                    self.apply_prompt();
                    let _ = self.model.needs_redraw.send(true);
                }
                Ok(Update::Redo) => {
                    if let Some(text) = self.conversation.redo_last() {
                        self.abort_requests();
                        let mut history = self.chat_history.lock().unwrap();
                        while let Some(msg) = history.pop() {
                            if matches!(msg, ChatMessage::User(_)) {
                                break;
                            }
                        }
                        drop(history);
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
            ConversationState::Thinking => format!("thinking… {}", self.spinner.step()),
            ConversationState::CallingTool(name) => format!("tool: {}", name),
            ConversationState::Responding => format!("responding… {}", self.spinner.step()),
        };
        let status_left = {
            let client = self.client.lock().unwrap();
            format!("{:?} {} {}", client.provider(), client.model(), state_text)
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
    needs_update: watch::Sender<bool>,
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
    needs_update: watch::Sender<bool>,
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
        let _ = self.needs_update.send(true);
        Ok(())
    }
}

struct PromptCommand {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
}

impl Command for PromptCommand {
    fn name(&self) -> &'static str {
        "prompt"
    }
    fn description(&self) -> &'static str {
        "Load a system prompt"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(PromptCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
            param: String::new(),
        })
    }
}

struct PromptCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
    param: String,
}

impl PromptCommandInstance {
    fn prompt_options(&self, typed: &str) -> Vec<Completion> {
        let mut names: Vec<String> = PromptAssets::iter()
            .filter_map(|f| {
                let name = f.as_ref();
                let name = name
                    .strip_suffix(".md")
                    .or_else(|| name.strip_suffix(".md.jinja"))?;
                if name.starts_with(typed) {
                    Some(name.to_string())
                } else {
                    None
                }
            })
            .collect();
        names.sort();
        names.dedup();
        names
            .into_iter()
            .map(|name| Completion {
                str: name.clone(),
                description: String::new(),
                name,
            })
            .collect()
    }
}

impl CommandInstance for PromptCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        self.param = input.trim().to_string();
        let options = self.prompt_options(self.param.as_str());
        CompletionResult::Options { at: 0, options }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.param.is_empty() {
            Err("no prompt".into())
        } else {
            let _ = self.update_tx.send(Update::SetPrompt(self.param.clone()));
            let _ = self.needs_update.send(true);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::load_prompt;

    #[test]
    fn load_md_prompt() {
        let content = load_prompt("sys/hello").unwrap();
        assert!(content.contains("You are a helpful assistant."));
    }

    #[test]
    fn load_md_jinja_with_include() {
        let content = load_prompt("sys/outer").unwrap();
        assert!(content.contains("Outer."));
        assert!(content.contains("Inner."));
        assert!(content.contains("Deep."));
    }

    #[test]
    fn load_md_jinja_with_glob() {
        let content = load_prompt("sys/glob").unwrap();
        assert!(content.contains("You are a helpful assistant."));
    }
}

struct QuitCommand {
    should_quit: watch::Sender<bool>,
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
    should_quit: watch::Sender<bool>,
}

impl CommandInstance for QuitCommandInstance {
    fn update(&mut self, _input: &str) -> CompletionResult {
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.should_quit.send(true);
        Ok(())
    }
}

struct RedoCommand {
    needs_update: watch::Sender<bool>,
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
    needs_update: watch::Sender<bool>,
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
        let _ = self.needs_update.send(true);
        Ok(())
    }
}

struct ClearCommand {
    needs_update: watch::Sender<bool>,
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
    needs_update: watch::Sender<bool>,
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
        let _ = self.needs_update.send(true);
        Ok(())
    }
}
