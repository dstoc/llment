use crate::{Id, Model};
use llm::MessageRole;
use tuirealm::props::{AttrValue, Attribute};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Quit,
    Clear,
    Redo,
}

impl SlashCommand {
    pub fn name(self) -> &'static str {
        match self {
            SlashCommand::Quit => "quit",
            SlashCommand::Clear => "clear",
            SlashCommand::Redo => "redo",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Quit => "Exit the application",
            SlashCommand::Clear => "Clear conversation history",
            SlashCommand::Redo => "Edit previous message",
        }
    }
}

pub fn matches(prefix: &str) -> Vec<SlashCommand> {
    [SlashCommand::Quit, SlashCommand::Clear, SlashCommand::Redo]
        .into_iter()
        .filter(|c| c.name().starts_with(prefix))
        .collect()
}

pub fn execute(cmd: SlashCommand, model: &mut Model) {
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
]
"###);
        insta::assert_debug_snapshot!(matches("c"), @r###"
[
    Clear,
]
"###);
        insta::assert_debug_snapshot!(matches("x"), @r###"[]"###);
    }
}
