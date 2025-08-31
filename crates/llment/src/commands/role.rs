use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, Completion, CompletionResult},
    prompts::Assets,
};

pub struct RoleCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for RoleCommand {
    fn name(&self) -> &'static str {
        "role"
    }
    fn description(&self) -> &'static str {
        "Set the assistant role"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(RoleCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
            param: String::new(),
        })
    }
}

struct RoleCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
    param: String,
}

impl RoleCommandInstance {
    fn role_options(&self, typed: &str) -> Vec<Completion> {
        let mut names: Vec<String> = Assets::iter()
            .filter_map(|f| {
                let name = f.as_ref();
                if !name.starts_with("roles/") {
                    return None;
                }
                let name = name.strip_prefix("roles/")?;
                let name = name.strip_suffix(".md")?;
                if name.starts_with(typed) {
                    Some(name.to_string())
                } else {
                    None
                }
            })
            .collect();
        if "none".starts_with(typed) {
            names.push("none".to_string());
        }
        names.sort();
        names.dedup();
        names
            .into_iter()
            .map(|name| Completion {
                str: name.clone(),
                description: String::new(),
                name,
            })
            .collect()
    }
}

impl CommandInstance for RoleCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        self.param = input.trim().to_string();
        let options = self.role_options(self.param.as_str());
        CompletionResult::Options { at: 0, options }
    }
    fn commit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.param.is_empty() {
            Err("no role".into())
        } else {
            let role = if self.param == "none" {
                None
            } else {
                Some(self.param.clone())
            };
            let _ = self.update_tx.send(Update::SetRole(role));
            let _ = self.needs_update.send(true);
            Ok(())
        }
    }
}
