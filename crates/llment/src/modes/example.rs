use super::{AgentMode, AgentModeStart, AgentModeStep};

pub struct ExampleAgentMode {
    stage: usize,
}

impl ExampleAgentMode {
    pub fn new() -> Self {
        Self { stage: 0 }
    }
}

impl AgentMode for ExampleAgentMode {
    fn start(&mut self) -> AgentModeStart {
        self.stage = 1;
        AgentModeStart {
            role: Some("swe".to_string()),
            prompt: "Hello from the example agent mode.".to_string(),
        }
    }

    fn step(&mut self) -> AgentModeStep {
        if self.stage == 1 {
            self.stage = 2;
            AgentModeStep {
                role: Some("swe".to_string()),
                prompt: Some("This is a follow-up from example agent mode.".to_string()),
            }
        } else {
            AgentModeStep {
                role: None,
                prompt: None,
            }
        }
    }
}
