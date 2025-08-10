use std::cmp::Ordering;

use llm_core::ResponseChunk;

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Chunk(ResponseChunk),
}

impl PartialEq for ChatEvent {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (ChatEvent::Chunk(_), ChatEvent::Chunk(_)))
    }
}

impl Eq for ChatEvent {}

impl PartialOrd for ChatEvent {
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        Some(Ordering::Equal)
    }
}

impl Ord for ChatEvent {
    fn cmp(&self, _other: &Self) -> Ordering {
        Ordering::Equal
    }
}
