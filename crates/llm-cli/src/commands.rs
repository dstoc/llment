use crate::{Id, Model};
use clap::ValueEnum;
use llm::{MessageRole, Provider};
use std::collections::HashMap;
use tuirealm::props::{AttrValue, Attribute, PropPayload, PropValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Quit,
    Clear,
    Redo,
    Model,
    Provider,
}

impl SlashCommand {
    pub fn name(self) -> &'static str {
        match self {
            SlashCommand::Quit => "quit",
            SlashCommand::Clear => "clear",
            SlashCommand::Redo => "redo",
            SlashCommand::Model => "model",
            SlashCommand::Provider => "provider",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Quit => "Exit the application",
            SlashCommand::Clear => "Clear conversation history",
            SlashCommand::Redo => "Edit previous message",
            SlashCommand::Model => "Change the active model",
            SlashCommand::Provider => "Change the provider and model",
        }
    }

    pub fn takes_param(self) -> bool {
        matches!(self, SlashCommand::Model | SlashCommand::Provider)
    }
}

pub fn matches(prefix: &str) -> Vec<SlashCommand> {
    [
        SlashCommand::Quit,
        SlashCommand::Clear,
        SlashCommand::Redo,
        SlashCommand::Model,
        SlashCommand::Provider,
    ]
    .into_iter()
    .filter(|c| c.name().starts_with(prefix))
    .collect()
}

fn provider_param_matches(
    prefix: &str,
    cache: &mut HashMap<Provider, Vec<String>>,
) -> (Vec<String>, Option<Provider>) {
    if let Some((prov, model_prefix)) = prefix.split_once(' ') {
        if let Ok(provider) = Provider::from_str(prov, true) {
            if let Some(models) = cache.get(&provider) {
                if models.len() == 1 && models[0] == "fetching..." {
                    (vec!["fetching...".to_string()], None)
                } else {
                    (
                        models
                            .iter()
                            .filter(|m| m.starts_with(model_prefix))
                            .cloned()
                            .collect(),
                        None,
                    )
                }
            } else {
                cache.insert(provider, vec!["fetching...".to_string()]);
                (vec!["fetching...".to_string()], Some(provider))
            }
        } else {
            (Vec::new(), None)
        }
    } else {
        (
            Provider::value_variants()
                .iter()
                .map(|p| p.to_possible_value().unwrap().get_name().to_string())
                .filter(|p| p.starts_with(prefix))
                .collect(),
            None,
        )
    }
}

pub fn param_matches(
    cmd: SlashCommand,
    prefix: &str,
    models: &[String],
    cache: &mut HashMap<Provider, Vec<String>>,
) -> (Vec<String>, Option<Provider>) {
    match cmd {
        SlashCommand::Model => {
            let ms = if models.len() == 1 && models[0] == "fetching..." {
                models.to_vec()
            } else {
                models
                    .iter()
                    .filter(|m| m.starts_with(prefix))
                    .cloned()
                    .collect()
            };
            (ms, None)
        }
        SlashCommand::Provider => provider_param_matches(prefix, cache),
        _ => (Vec::new(), None),
    }
}

pub fn parse(input: &str) -> Option<(SlashCommand, Option<String>)> {
    if !input.starts_with('/') {
        return None;
    }
    let rest = &input[1..];
    if let Some((name, param)) = rest.split_once(' ') {
        let ms = matches(name);
        if ms.len() == 1 && ms[0].name() == name {
            let p = if param.is_empty() {
                None
            } else {
                Some(param.to_string())
            };
            Some((ms[0], p))
        } else {
            None
        }
    } else {
        let ms = matches(rest);
        if ms.len() == 1 && ms[0].name() == rest {
            Some((ms[0], None))
        } else {
            None
        }
    }
}

pub fn execute(cmd: SlashCommand, param: Option<String>, model: &mut Model) {
    match cmd {
        SlashCommand::Quit => model.quit = true,
        SlashCommand::Clear => {
            model.conversation.borrow_mut().clear();
            model.chat_history.clear();
        }
        SlashCommand::Redo => {
            if let Some(text) = model.conversation.borrow_mut().redo_last() {
                while let Some(msg) = model.chat_history.pop() {
                    if msg.role == MessageRole::User {
                        break;
                    }
                }
                let _ = model
                    .app
                    .attr(&Id::Input, Attribute::Text, AttrValue::String(text));
                let _ = model.app.active(&Id::Input);
                let _ = model
                    .app
                    .attr(&Id::Input, Attribute::Focus, AttrValue::Flag(true));
                model.tool_stream = None;
                if let Some(handle) = model.tool_task.take() {
                    handle.abort();
                }
                model.pending_tools.clear();
            }
        }
        SlashCommand::Model => {
            if let Some(name) = param {
                model.model_name = name;
            }
        }
        SlashCommand::Provider => {
            if let Some(param) = param {
                if let Some((prov, model_name)) = param.split_once(' ') {
                    if let Ok(provider) = Provider::from_str(prov, true) {
                        let client = llm::client_from(provider, &model.host).expect("client");
                        model.client = client.clone();
                        model.provider = provider;
                        model.model_name = model_name.to_string();
                        let prov_name =
                            provider.to_possible_value().unwrap().get_name().to_string();
                        let _ = model.app.attr(
                            &Id::Input,
                            Attribute::Custom("provider"),
                            AttrValue::String(prov_name.clone()),
                        );
                        let models_attr = if let Some(Some(models)) = model.models.get(&provider) {
                            AttrValue::Payload(PropPayload::Vec(
                                std::iter::once(PropValue::Str(prov_name.clone()))
                                    .chain(models.iter().cloned().map(PropValue::Str))
                                    .collect(),
                            ))
                        } else {
                            // kick off async fetch if not already started
                            if model
                                .model_fetch
                                .as_ref()
                                .map(|(p, _)| *p != provider)
                                .unwrap_or(true)
                            {
                                if let Some((_, handle)) = model.model_fetch.take() {
                                    handle.abort();
                                }
                                let fetch_client = client.clone();
                                model.model_fetch = Some((
                                    provider,
                                    tokio::spawn(async move {
                                        fetch_client.list_models().await.unwrap_or_default()
                                    }),
                                ));
                            }
                            model.models.entry(provider).or_insert(None);
                            AttrValue::Payload(PropPayload::Vec(vec![
                                PropValue::Str(prov_name.clone()),
                                PropValue::Str("fetching...".to_string()),
                            ]))
                        };
                        let _ =
                            model
                                .app
                                .attr(&Id::Input, Attribute::Custom("models"), models_attr);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_prefixes() {
        insta::assert_debug_snapshot!(matches(""), @r###"
[
    Quit,
    Clear,
    Redo,
    Model,
    Provider,
]
"###);
        insta::assert_debug_snapshot!(matches("c"), @r###"
[
    Clear,
]
"###);
        insta::assert_debug_snapshot!(matches("m"), @r###"
[
    Model,
]
"###);
        insta::assert_debug_snapshot!(matches("p"), @r###"
[
    Provider,
]
"###);
        insta::assert_debug_snapshot!(matches("x"), @r###"[]"###);
    }
}
