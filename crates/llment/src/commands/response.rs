use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, CompletionResult},
    history_edits,
};

pub struct ResponseCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for ResponseCommand {
    fn name(&self) -> &'static str {
        "response"
    }
    fn description(&self) -> &'static str {
        "Append a response message to the conversation"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(ResponseCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
            param: String::new(),
        })
    }
}

struct ResponseCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
    param: String,
}

impl CommandInstance for ResponseCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        self.param = input.to_string();
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }

    fn commit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let edit = history_edits::append_response(self.param.clone());
        let _ = self.update_tx.send(Update::EditHistory(edit));
        let _ = self.needs_update.send(true);
        Ok(())
    }
}
