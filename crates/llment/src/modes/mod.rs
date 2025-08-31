use llm::mcp::McpService;
use rmcp::service::{RoleClient, RunningService};

pub struct AgentModeStart {
    pub role: Option<String>,
    pub prompt: String,
}

pub struct AgentModeStep {
    pub role: Option<String>,
    pub prompt: Option<String>,
}

pub trait AgentMode: Send {
    fn start(&mut self) -> AgentModeStart;
    fn step(&mut self) -> AgentModeStep;
    fn service_prefix(&self) -> Option<&str> {
        None
    }
}

pub async fn create_agent_mode(
    name: &str,
) -> Option<(
    Box<dyn AgentMode>,
    Option<RunningService<RoleClient, McpService>>,
)> {
    match name {
        "example" => Some((Box::new(example::ExampleAgentMode::new()), None)),
        _ => None,
    }
}

pub fn available_modes() -> Vec<&'static str> {
    vec!["example"]
}

mod example;
