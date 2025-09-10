use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, CompletionResult},
};

pub struct PopCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for PopCommand {
    fn name(&self) -> &'static str {
        "pop"
    }
    fn description(&self) -> &'static str {
        "Remove the last assistant response part or message"
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(PopCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
        })
    }
}

struct PopCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
}

impl CommandInstance for PopCommandInstance {
    fn update(&mut self, _input: &str) -> CompletionResult {
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }
    fn commit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.update_tx.send(Update::Pop);
        let _ = self.needs_update.send(true);
        Ok(())
    }
}
