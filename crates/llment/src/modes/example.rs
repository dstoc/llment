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
            prompt: Some("Hello from the example agent mode.".to_string()),
            clear_history: true,
        }
    }

    fn step(&mut self) -> AgentModeStep {
        if self.stage == 1 {
            self.stage = 2;
            AgentModeStep {
                role: Some("swe".to_string()),
                prompt: Some("This is a follow-up from example agent mode.".to_string()),
                clear_history: false,
                stop: false,
            }
        } else {
            AgentModeStep {
                role: None,
                prompt: None,
                clear_history: false,
                stop: true,
            }
        }
    }
}
