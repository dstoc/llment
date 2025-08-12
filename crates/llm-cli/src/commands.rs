use crate::Model;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Quit,
    Clear,
}

impl SlashCommand {
    pub fn name(self) -> &'static str {
        match self {
            SlashCommand::Quit => "quit",
            SlashCommand::Clear => "clear",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Quit => "Exit the application",
            SlashCommand::Clear => "Clear conversation history",
        }
    }
}

pub fn matches(prefix: &str) -> Vec<SlashCommand> {
    [SlashCommand::Quit, SlashCommand::Clear]
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
    }
}
