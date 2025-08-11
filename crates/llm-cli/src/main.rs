use std::cell::RefCell;
use std::collections::HashMap;
use std::io::stdout;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;

use futures::FutureExt;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use tokio_stream::{Stream, StreamExt};
use tuirealm::application::PollStrategy;
use tuirealm::ratatui::layout::{Constraint, Direction as LayoutDirection, Layout};
use tuirealm::terminal::{CrosstermTerminalAdapter, TerminalBridge};
use tuirealm::{
    Application, Attribute, EventListenerCfg, NoUserEvent, Sub, SubClause, SubEventClause, Update,
    props::AttrValue,
};

mod components;
mod conversation;
mod markdown;
use components::Prompt;
use conversation::{Conversation, Node, ToolStep};

use llm::mcp::{McpContext, McpToolExecutor};
use llm::tools::{self, ToolEvent, ToolExecutor};
use llm::{self, ChatMessage, ChatMessageRequest, Provider};

#[derive(Debug, PartialEq)]
pub enum Msg {
    AppClose,
    FocusConversation,
    FocusInput,
    Submit(String),
    None,
}

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub enum Id {
    Conversation,
    Input,
}

struct ConvComponent(Rc<RefCell<Conversation>>);

impl tuirealm::MockComponent for ConvComponent {
    fn view(
        &mut self,
        frame: &mut tuirealm::ratatui::Frame,
        area: tuirealm::ratatui::layout::Rect,
    ) {
        self.0.borrow_mut().view(frame, area);
    }

    fn query(&self, attr: Attribute) -> Option<AttrValue> {
        self.0.borrow().query(attr)
    }

    fn attr(&mut self, attr: Attribute, value: AttrValue) {
        self.0.borrow_mut().attr(attr, value);
    }

    fn state(&self) -> tuirealm::State {
        self.0.borrow().state()
    }

    fn perform(&mut self, cmd: tuirealm::command::Cmd) -> tuirealm::command::CmdResult {
        self.0.borrow_mut().perform(cmd)
    }
}

impl tuirealm::Component<Msg, NoUserEvent> for ConvComponent {
    fn on(&mut self, ev: tuirealm::Event<NoUserEvent>) -> Option<Msg> {
        self.0.borrow_mut().on(ev)
    }
}

struct Model {
    app: Application<Id, Msg, NoUserEvent>,
    quit: bool,
    redraw: bool,
    conversation: Rc<RefCell<Conversation>>,
    chat_history: Vec<ChatMessage>,
    client: Arc<dyn llm::LlmClient>,
    model_name: String,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_stream: Option<Box<dyn Stream<Item = ToolEvent> + Unpin + Send>>,
    tool_task:
        Option<JoinHandle<Result<Vec<ChatMessage>, Box<dyn std::error::Error + Send + Sync>>>>,
    pending_tools: HashMap<usize, usize>,
    runtime: Runtime,
}

impl Default for Model {
    fn default() -> Self {
        let runtime = Runtime::new().expect("runtime");
        let mut app: Application<Id, Msg, NoUserEvent> = Application::init(
            EventListenerCfg::default().crossterm_input_listener(Duration::from_millis(10), 10),
        );
        let conversation = Rc::new(RefCell::new(Conversation::default()));
        assert!(
            app.mount(
                Id::Conversation,
                Box::new(ConvComponent(conversation.clone())),
                vec![Sub::new(SubEventClause::Any, SubClause::Always)],
            )
            .is_ok()
        );
        assert!(
            app.mount(
                Id::Input,
                Box::new(Prompt::default()),
                vec![Sub::new(SubEventClause::Any, SubClause::Always)],
            )
            .is_ok()
        );
        assert!(app.active(&Id::Input).is_ok());
        let client = llm::client_from(Provider::Ollama, "http://127.0.0.1:11434").expect("client");
        let mcp_ctx = Arc::new(McpContext::default());
        let tool_executor: Arc<dyn ToolExecutor> = Arc::new(McpToolExecutor::new(mcp_ctx));
        Self {
            app,
            quit: false,
            redraw: true,
            conversation,
            chat_history: Vec::new(),
            client,
            model_name: "gpt-oss:20b".into(),
            tool_executor,
            tool_stream: None,
            tool_task: None,
            pending_tools: HashMap::new(),
            runtime,
        }
    }
}

impl Model {
    fn view(&mut self, terminal: &mut TerminalBridge<CrosstermTerminalAdapter>) {
        let _ = terminal.raw_mut().draw(|f| {
            let area = f.area();
            let input_height = self
                .app
                .query(&Id::Input, Attribute::Height)
                .ok()
                .flatten()
                .and_then(|v| match v {
                    AttrValue::Length(l) => Some(l as u16),
                    AttrValue::Size(s) => Some(s),
                    _ => None,
                })
                .unwrap_or(3);
            let chunks = Layout::default()
                .direction(LayoutDirection::Vertical)
                .margin(1)
                .constraints([Constraint::Min(1), Constraint::Length(input_height)].as_ref())
                .split(area);
            self.app.view(&Id::Conversation, f, chunks[0]);
            self.app.view(&Id::Input, f, chunks[1]);
        });
    }
}

impl Update<Msg> for Model {
    fn update(&mut self, msg: Option<Msg>) -> Option<Msg> {
        self.redraw = true;
        match msg.unwrap_or(Msg::None) {
            Msg::AppClose => {
                self.quit = true;
                None
            }
            Msg::FocusConversation => {
                let _ = self.app.active(&Id::Conversation);
                None
            }
            Msg::FocusInput => {
                let _ = self.app.active(&Id::Input);
                None
            }
            Msg::Submit(text) => {
                self.conversation.borrow_mut().push_user(text.clone());
                self.chat_history.push(ChatMessage::user(text));
                self.conversation.borrow_mut().push_assistant_block();
                let request =
                    ChatMessageRequest::new(self.model_name.clone(), self.chat_history.clone())
                        .think(true);
                let history = std::mem::take(&mut self.chat_history);
                let (stream, handle) = {
                    let _guard = self.runtime.enter();
                    tools::tool_event_stream(
                        self.client.clone(),
                        request,
                        self.tool_executor.clone(),
                        history,
                    )
                };
                self.tool_stream = Some(Box::new(stream));
                self.tool_task = Some(handle);
                self.pending_tools.clear();
                None
            }
            Msg::None => None,
        }
    }
}

fn main() {
    let mut model = Model::default();
    let mut terminal = TerminalBridge::init_crossterm().expect("Cannot create terminal bridge");
    let _ = terminal.enable_raw_mode();
    let _ = terminal.enter_alternate_screen();
    let _ = execute!(stdout(), EnableMouseCapture);

    while !model.quit {
        if let Ok(messages) = model.app.tick(PollStrategy::Once) {
            for msg in messages {
                let mut current = Some(msg);
                while let Some(m) = current {
                    current = model.update(Some(m));
                }
            }
        }
        if let Some(stream) = &mut model.tool_stream {
            if let Some(ev) = model
                .runtime
                .block_on(async { stream.next().now_or_never() })
                .flatten()
            {
                match ev {
                    ToolEvent::Chunk(chunk) => {
                        if let Some(thinking) = chunk.message.thinking.as_ref() {
                            model.conversation.borrow_mut().append_thinking(thinking);
                        }
                        if let Some(content) = chunk.message.content.as_ref() {
                            if !content.is_empty() {
                                model.conversation.borrow_mut().append_response(content);
                            }
                        }
                    }
                    ToolEvent::ToolStarted { id, name, args } => {
                        let step =
                            Node::Tool(ToolStep::new(name, args.to_string(), String::new(), true));
                        let idx = model.conversation.borrow_mut().add_step(step);
                        model.pending_tools.insert(id, idx);
                    }
                    ToolEvent::ToolResult { id, result, .. } => {
                        if let Some(idx) = model.pending_tools.remove(&id) {
                            let text = result.unwrap_or_else(|e| format!("Tool Failed: {}", e));
                            model
                                .conversation
                                .borrow_mut()
                                .update_tool_result(idx, text);
                        }
                    }
                }
                model.redraw = true;
            }
        }
        if let Some(handle) = &mut model.tool_task {
            if let Some(res) = model.runtime.block_on(async { handle.now_or_never() }) {
                match res {
                    Ok(Ok(history)) => model.chat_history = history,
                    Ok(Err(err)) => model
                        .conversation
                        .borrow_mut()
                        .append_response(&format!("Error: {}", err)),
                    Err(err) => model
                        .conversation
                        .borrow_mut()
                        .append_response(&format!("Error: {}", err)),
                }
                model.tool_stream = None;
                model.tool_task = None;
                model.redraw = true;
            }
        }
        if model.redraw {
            model.view(&mut terminal);
            model.redraw = false;
        }
    }

    let _ = execute!(stdout(), DisableMouseCapture);
    let _ = terminal.leave_alternate_screen();
    let _ = terminal.disable_raw_mode();
    let _ = terminal.clear_screen();
}
