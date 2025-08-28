use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::{Assets, Update},
    components::completion::{Command, CommandInstance, Completion, CompletionResult},
};

pub struct PromptCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for PromptCommand {
    fn name(&self) -> &'static str {
        "prompt"
    }
    fn description(&self) -> &'static str {
        "Load a system prompt"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(PromptCommandInstance {
            needs_update: self.needs_update.clone(),
            update_tx: self.update_tx.clone(),
            param: String::new(),
        })
    }
}

struct PromptCommandInstance {
    needs_update: watch::Sender<bool>,
    update_tx: UnboundedSender<Update>,
    param: String,
}

impl PromptCommandInstance {
    fn prompt_options(&self, typed: &str) -> Vec<Completion> {
        let mut names: Vec<String> = Assets::iter()
            .filter_map(|f| {
                let name = f.as_ref();
                let name = name
                    .strip_suffix(".md")
                    .or_else(|| name.strip_suffix(".md.jinja"))?;
                if name.starts_with(typed) {
                    Some(name.to_string())
                } else {
                    None
                }
            })
            .collect();
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

impl CommandInstance for PromptCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        self.param = input.trim().to_string();
        let options = self.prompt_options(self.param.as_str());
        CompletionResult::Options { at: 0, options }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.param.is_empty() {
            Err("no prompt".into())
        } else {
            let _ = self.update_tx.send(Update::SetPrompt(self.param.clone()));
            let _ = self.needs_update.send(true);
            Ok(())
        }
    }
}
