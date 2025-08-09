pub mod assistant;
pub mod error;
pub mod separator;
pub mod thinking;
pub mod user;

pub use assistant::AssistantItem;
pub use error::ErrorItem;
pub use separator::SeparatorItem;
pub use thinking::{ThinkingItem, ThinkingStep};
pub use user::UserItem;

pub enum HistoryItem {
    User(UserItem),
    Assistant(AssistantItem),
    Thinking(ThinkingItem),
    Separator(SeparatorItem),
    Error(ErrorItem),
}
