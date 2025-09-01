use llm::mcp::McpService;
use rmcp::service::{RoleClient, RunningService};

pub struct AgentModeStart {
    pub role: Option<String>,
    pub prompt: Option<String>,
    pub clear_history: bool,
}

pub struct AgentModeStep {
    pub role: Option<String>,
    pub prompt: Option<String>,
    pub clear_history: bool,
    pub stop: bool,
}

pub trait AgentMode: Send {
    fn start(&mut self) -> AgentModeStart;
    fn step(&mut self) -> AgentModeStep;
}

pub async fn create_agent_mode(
    name: &str,
) -> Option<(
    Box<dyn AgentMode>,
    Option<RunningService<RoleClient, McpService>>,
)> {
    match name {
        "code-agent" => {
            let (mode, service) = code_agent::CodeAgentMode::new().await;
            Some((Box::new(mode), Some(service)))
        }
        "example" => Some((Box::new(example::ExampleAgentMode::new()), None)),
        _ => None,
    }
}

pub fn available_modes() -> Vec<&'static str> {
    vec!["code-agent", "example"]
}

mod code_agent;
mod example;
