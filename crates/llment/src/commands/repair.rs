use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, CompletionResult},
};

pub struct RepairCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for RepairCommand {
    fn name(&self) -> &'static str {
        "repair"
    }
    fn description(&self) -> &'static str {
        "Remove empty assistant blocks"
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(RepairCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
        })
    }
}

struct RepairCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
}

impl CommandInstance for RepairCommandInstance {
    fn update(&mut self, _input: &str) -> CompletionResult {
        CompletionResult::Options {
            at: 0,
            options: vec![],
        }
    }
    fn commit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.update_tx.send(Update::Repair);
        let _ = self.needs_update.send(true);
        Ok(())
    }
}
