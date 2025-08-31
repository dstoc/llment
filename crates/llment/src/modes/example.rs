use super::AgentMode;

pub struct ExampleAgentMode {
    stage: usize,
}

impl ExampleAgentMode {
    pub fn new() -> Self {
        Self { stage: 0 }
    }
}

impl AgentMode for ExampleAgentMode {
    fn start(&mut self) -> (Option<String>, String) {
        self.stage = 1;
        (
            Some("swe".to_string()),
            "Hello from the example agent mode.".to_string(),
        )
    }

    fn step(&mut self) -> (Option<String>, Option<String>) {
        if self.stage == 1 {
            self.stage = 2;
            (
                Some("swe".to_string()),
                Some("This is a follow-up from example agent mode.".to_string()),
            )
        } else {
            (None, None)
        }
    }
}
