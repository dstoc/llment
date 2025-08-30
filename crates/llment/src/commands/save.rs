use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, CompletionResult},
};

pub struct SaveCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for SaveCommand {
    fn name(&self) -> &'static str {
        "save"
    }
    fn description(&self) -> &'static str {
        "Save the conversation history to a file"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(SaveCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
            param: String::new(),
        })
    }
}

struct SaveCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
    param: String,
}

impl CommandInstance for SaveCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        self.param = input.trim().to_string();
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }

    fn commit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.param.is_empty() {
            Err("no filename".into())
        } else {
            let _ = self.update_tx.send(Update::Save(self.param.clone()));
            let _ = self.needs_update.send(true);
            Ok(())
        }
    }
}
