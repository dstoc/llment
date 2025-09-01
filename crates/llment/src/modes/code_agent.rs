use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use llm::{ToolInfo, mcp::McpService};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::{ServerCapabilities, ServerInfo},
    service::{RoleClient, RunningService, ServiceExt},
    tool, tool_handler, tool_router,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use tokio::io::duplex;

use super::{AgentMode, AgentModeStart, AgentModeStep};

#[derive(Default)]
struct NotifyState {
    role: Option<String>,
    message: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum CodeAgentRole {
    Director,
    DesignLead,
    ExecutionLead,
    EngTeam,
    Reviewer,
}

impl CodeAgentRole {
    fn as_str(&self) -> &'static str {
        match self {
            CodeAgentRole::Director => "director",
            CodeAgentRole::DesignLead => "design-lead",
            CodeAgentRole::ExecutionLead => "execution-lead",
            CodeAgentRole::EngTeam => "eng-team",
            CodeAgentRole::Reviewer => "reviewer",
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct NotifyParams {
    role: CodeAgentRole,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Clone)]
struct CodeAgentTools {
    state: Arc<Mutex<NotifyState>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl CodeAgentTools {
    fn new(state: Arc<Mutex<NotifyState>>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "notify",
        description = "Switch to another code-agent role with an optional message"
    )]
    fn notify(&self, Parameters(params): Parameters<NotifyParams>) -> String {
        let mut state = self.state.lock().unwrap();
        state.role = Some(params.role.as_str().to_string());
        state.message = params.message;
        "ok".into()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for CodeAgentTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub struct CodeAgentMode {
    current_role: String,
    state: Arc<Mutex<NotifyState>>,
}

impl CodeAgentMode {
    pub async fn new() -> (Self, RunningService<RoleClient, McpService>) {
        let state = Arc::new(Mutex::new(NotifyState::default()));
        let tools = CodeAgentTools::new(state.clone());
        let (server_transport, client_transport) = duplex(64);
        let (server_res, client_res) = tokio::join!(
            tools.clone().serve(server_transport),
            McpService {
                prefix: "agent".into(),
                tools: ArcSwap::new(Arc::new(vec![ToolInfo {
                    name: "notify".into(),
                    description: "Switch to another code-agent role with an optional message"
                        .into(),
                    parameters: schema_for!(NotifyParams),
                }])),
            }
            .serve(client_transport)
        );
        let server = server_res.expect("code agent server");
        let client_service = client_res.expect("code agent client");
        tokio::spawn(async move {
            let _ = server.waiting().await;
        });
        (
            Self {
                current_role: "code-agent/director".to_string(),
                state,
            },
            client_service,
        )
    }
}

impl AgentMode for CodeAgentMode {
    fn start(&mut self) -> AgentModeStart {
        AgentModeStart {
            role: Some(self.current_role.clone()),
            prompt: Some("Let's begin.".to_string()),
            clear_history: true,
        }
    }

    fn step(&mut self) -> AgentModeStep {
        let mut state = self.state.lock().unwrap();
        if let Some(role) = state.role.take() {
            match role.as_str() {
                "director" | "design-lead" | "execution-lead" | "eng-team" | "reviewer" => {
                    self.current_role = format!("code-agent/{}", role);
                    AgentModeStep {
                        role: Some(self.current_role.clone()),
                        prompt: state.message.take(),
                        clear_history: false,
                        stop: false,
                    }
                }
                _ => AgentModeStep {
                    role: Some(self.current_role.clone()),
                    prompt: Some(
                        "Please call agent.notify(role, message) to continue.".to_string(),
                    ),
                    clear_history: false,
                    stop: false,
                },
            }
        } else {
            AgentModeStep {
                role: Some(self.current_role.clone()),
                prompt: Some("Please call agent.notify(role, message) to continue.".to_string()),
                clear_history: false,
                stop: false,
            }
        }
    }
}
