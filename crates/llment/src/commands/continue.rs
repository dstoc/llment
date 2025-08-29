use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, CompletionResult},
};

pub struct ContinueCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for ContinueCommand {
    fn name(&self) -> &'static str {
        "continue"
    }
    fn description(&self) -> &'static str {
        "Request the model to continue the last response"
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(ContinueCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
        })
    }
}

struct ContinueCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
}

impl CommandInstance for ContinueCommandInstance {
    fn update(&mut self, _input: &str) -> CompletionResult {
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.update_tx.send(Update::Continue);
        let _ = self.needs_update.send(true);
        Ok(())
    }
}
