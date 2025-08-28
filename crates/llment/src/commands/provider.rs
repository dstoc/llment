use clap::ValueEnum;
use llm::Provider;
use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, Completion, CompletionResult},
};

pub struct ProviderCommand {
    pub(crate) needs_update: watch::Sender<bool>,
    pub(crate) update_tx: UnboundedSender<Update>,
}

impl Command for ProviderCommand {
    fn name(&self) -> &'static str {
        "provider"
    }
    fn description(&self) -> &'static str {
        "Change the active provider"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(ProviderCommandInstance {
            needs_update: self.needs_update.clone(),
            tx: self.update_tx.clone(),
            param: String::new(),
        })
    }
}

struct ProviderCommandInstance {
    needs_update: watch::Sender<bool>,
    tx: UnboundedSender<Update>,
    param: String,
}

impl ProviderCommandInstance {
    fn provider_options(&self, typed: &str) -> Vec<Completion> {
        Provider::value_variants()
            .iter()
            .filter_map(|p| {
                let name = p.to_possible_value()?.get_name().to_string();
                if name.starts_with(typed) {
                    Some(Completion {
                        name: name.clone(),
                        description: String::new(),
                        str: format!("{} ", name),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

impl CommandInstance for ProviderCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        self.param = input.trim().to_string();
        let (prov, host_opt) = match self.param.split_once(' ') {
            Some((p, h)) => (p, Some(h)),
            None => (self.param.as_str(), None),
        };
        if host_opt.is_none() {
            let options = self.provider_options(prov);
            CompletionResult::Options { at: 0, options }
        } else {
            CompletionResult::Options {
                at: 0,
                options: vec![],
            }
        }
    }
    fn commit(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.param.is_empty() {
            return Err("no provider".into());
        }
        let mut parts = self.param.split_whitespace();
        let prov_str = parts.next().ok_or("no provider")?;
        let provider = Provider::from_str(prov_str, true)?;
        let host = parts.next().map(|s| s.to_string());
        let _ = self.tx.send(Update::SetProvider(provider, host));
        let _ = self.needs_update.send(true);
        Ok(())
    }
}
