use tokio::sync::watch;

use crate::components::completion::{Command, CommandInstance, CompletionResult};

pub struct QuitCommand {
    pub(crate) should_quit: watch::Sender<bool>,
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
