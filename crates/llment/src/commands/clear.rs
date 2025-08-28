use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, CompletionResult},
};

pub struct ClearCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for ClearCommand {
    fn name(&self) -> &'static str {
        "clear"
    }
    fn description(&self) -> &'static str {
        "Clear the conversation history"
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(ClearCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
        })
    }
}

struct ClearCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
}

impl CommandInstance for ClearCommandInstance {
    fn update(&mut self, _input: &str) -> CompletionResult {
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.update_tx.send(Update::Clear);
        let _ = self.needs_update.send(true);
        Ok(())
    }
}
