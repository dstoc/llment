use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use llm::{ChatMessage, ToolInfo, mcp::McpService};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::{ServerCapabilities, ServerInfo},
    service::{RoleClient, RunningService, ServiceExt},
    tool, tool_handler, tool_router,
};
use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Serialize};
use tokio::io::duplex;

use super::{AgentMode, AgentModeStart, AgentModeStep};

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy)]
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

#[derive(Default)]
struct NotifyState {
    role: Option<CodeAgentRole>,
    message: Option<String>,
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
        state.role = Some(params.role);
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
    current_role: CodeAgentRole,
    state: Arc<Mutex<NotifyState>>,
}

impl CodeAgentMode {
    pub async fn new() -> (Self, RunningService<RoleClient, McpService>) {
        let state = Arc::new(Mutex::new(NotifyState::default()));
        let tools = CodeAgentTools::new(state.clone());
        let tool_infos: Vec<ToolInfo> = tools
            .tool_router
            .list_all()
            .into_iter()
            .filter_map(|tool| {
                let schema: Schema = serde_json::from_value(tool.schema_as_json_value()).ok()?;
                let description = tool.description.unwrap_or_default().to_string();
                Some(ToolInfo {
                    name: tool.name.to_string(),
                    description,
                    parameters: schema,
                })
            })
            .collect();
        let (server_transport, client_transport) = duplex(64);
        let (server_res, client_res) = tokio::join!(
            tools.clone().serve(server_transport),
            McpService {
                prefix: "agent".into(),
                tools: ArcSwap::new(Arc::new(tool_infos)),
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
                current_role: CodeAgentRole::Director,
                state,
            },
            client_service,
        )
    }
}

impl AgentMode for CodeAgentMode {
    fn start(&mut self) -> AgentModeStart {
        AgentModeStart {
            role: Some(format!("code-agent/{}", self.current_role.as_str())),
            prompt: None,
            clear_history: true,
        }
    }

    fn step(&mut self, _last_message: Option<&ChatMessage>) -> AgentModeStep {
        let mut state = self.state.lock().unwrap();
        if let Some(role) = state.role.take() {
            self.current_role = role;
            AgentModeStep {
                role: Some(format!("code-agent/{}", self.current_role.as_str())),
                prompt: state.message.take(),
                clear_history: true,
                stop: false,
            }
        } else if matches!(self.current_role, CodeAgentRole::Director) {
            AgentModeStep {
                role: Some(format!("code-agent/{}", self.current_role.as_str())),
                prompt: None,
                clear_history: false,
                stop: true,
            }
        } else {
            AgentModeStep {
                role: Some(format!("code-agent/{}", self.current_role.as_str())),
                prompt: Some(
                    "Please finish your job and then call agent_notify(role, message) as requested.".to_string(),
                ),
                clear_history: false,
                stop: false,
            }
        }
    }
}
