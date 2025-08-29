use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, CompletionResult},
};

pub struct RedoCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for RedoCommand {
    fn name(&self) -> &'static str {
        "redo"
    }
    fn description(&self) -> &'static str {
        "Rewrite the last prompt"
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(RedoCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
        })
    }
}

struct RedoCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
}

impl CommandInstance for RedoCommandInstance {
    fn update(&mut self, _input: &str) -> CompletionResult {
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }
    fn commit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.update_tx.send(Update::Redo);
        let _ = self.needs_update.send(true);
        Ok(())
    }
}
