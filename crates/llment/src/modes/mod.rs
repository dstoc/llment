use llm::mcp::McpService;
use rmcp::service::{RoleClient, RunningService};

pub trait AgentMode: Send {
    fn start(&mut self) -> (String, String);
    fn step(&mut self) -> (String, Option<String>);
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
