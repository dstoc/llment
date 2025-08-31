use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, Completion, CompletionResult},
    modes,
};

pub struct AgentModeCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for AgentModeCommand {
    fn name(&self) -> &'static str {
        "agent-mode"
    }
    fn description(&self) -> &'static str {
        "Activate or deactivate an agent mode"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(AgentModeCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
            param: String::new(),
        })
    }
}

struct AgentModeCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
    param: String,
}

impl AgentModeCommandInstance {
    fn mode_options(&self, typed: &str) -> Vec<Completion> {
        let mut options: Vec<Completion> = modes::available_modes()
            .into_iter()
            .chain(std::iter::once("off"))
            .filter(|m| m.starts_with(typed))
            .map(|m| Completion {
                name: m.to_string(),
                description: String::new(),
                str: m.to_string(),
            })
            .collect();
        options.sort_by(|a, b| a.name.cmp(&b.name));
        options
    }
}

impl CommandInstance for AgentModeCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        self.param = input.trim().to_string();
        let options = self.mode_options(self.param.as_str());
        CompletionResult::Options { at: 0, options }
    }
    fn commit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.param.is_empty() {
            Err("no mode".into())
        } else if self.param == "off" {
            let tx = self.update_tx.clone();
            let needs_update = self.needs_update.clone();
            tokio::spawn(async move {
                let _ = tx.send(Update::Clear);
                let _ = tx.send(Update::SetMode(None, None));
                let _ = needs_update.send(true);
            });
            Ok(())
        } else {
            let mode_name = self.param.clone();
            let tx = self.update_tx.clone();
            let needs_update = self.needs_update.clone();
            tokio::spawn(async move {
                if let Some((mode, service)) = modes::create_agent_mode(&mode_name).await {
                    let _ = tx.send(Update::Clear);
                    let _ = tx.send(Update::SetMode(Some(mode), service));
                    let _ = needs_update.send(true);
                }
            });
            Ok(())
        }
    }
}
