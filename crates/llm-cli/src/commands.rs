use crate::{Id, Model};
use llm::MessageRole;
use tuirealm::props::{AttrValue, Attribute};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Quit,
    Clear,
    Redo,
    Model,
}

impl SlashCommand {
    pub fn name(self) -> &'static str {
        match self {
            SlashCommand::Quit => "quit",
            SlashCommand::Clear => "clear",
            SlashCommand::Redo => "redo",
            SlashCommand::Model => "model",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Quit => "Exit the application",
            SlashCommand::Clear => "Clear conversation history",
            SlashCommand::Redo => "Edit previous message",
            SlashCommand::Model => "Change the active model",
        }
    }

    pub fn takes_param(self) -> bool {
        matches!(self, SlashCommand::Model)
    }
}

pub fn matches(prefix: &str) -> Vec<SlashCommand> {
    [
        SlashCommand::Quit,
        SlashCommand::Clear,
        SlashCommand::Redo,
        SlashCommand::Model,
    ]
    .into_iter()
    .filter(|c| c.name().starts_with(prefix))
    .collect()
}

pub fn param_matches(cmd: SlashCommand, prefix: &str, models: &[String]) -> Vec<String> {
    match cmd {
        SlashCommand::Model => models
            .iter()
            .filter(|m| m.starts_with(prefix))
            .cloned()
            .collect(),
        _ => Vec::new(),
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
        insta::assert_debug_snapshot!(matches("x"), @r###"[]"###);
    }
}
