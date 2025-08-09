pub mod chat;
pub mod input;

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub enum Id {
    Chat,
    Input,
}
