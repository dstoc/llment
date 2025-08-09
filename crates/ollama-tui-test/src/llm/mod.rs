use std::error::Error;
use std::pin::Pin;

use async_trait::async_trait;
use tokio_stream::Stream;

pub use ollama_rs::{
    generation::{
        chat::{ChatMessage, MessageRole, request::ChatMessageRequest},
        tools::{ToolCall, ToolCallFunction, ToolFunctionInfo, ToolInfo, ToolType},
    },
    re_exports::schemars::Schema,
};

pub mod gemini;
pub mod ollama;
pub mod openai;

#[derive(Debug)]
pub struct ResponseMessage {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub thinking: Option<String>,
}

#[derive(Debug)]
pub struct ResponseChunk {
    pub message: ResponseMessage,
    pub done: bool,
}

pub type ChatStream =
    Pin<Box<dyn Stream<Item = Result<ResponseChunk, Box<dyn Error + Send + Sync>>> + Send>>;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>>;
}
