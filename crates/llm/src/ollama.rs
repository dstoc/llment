use std::error::Error;

use async_trait::async_trait;
use ollama_rs::Ollama;
use ollama_rs::generation::chat::{ChatMessageResponseStream, request::ChatMessageRequest};
use tokio_stream::StreamExt;

use super::{ChatStream, LlmClient, ResponseChunk, ResponseMessage, Usage};

pub struct OllamaClient {
    inner: Ollama,
}

impl OllamaClient {
    pub fn new(host: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Ok(Self {
            inner: Ollama::try_new(host)?,
        })
    }
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let stream: ChatMessageResponseStream =
            self.inner.send_chat_messages_stream(request).await?;
        let mapped = stream.map(|res| match res {
            Ok(r) => Ok(ResponseChunk {
                message: ResponseMessage {
                    content: if r.message.content.is_empty() {
                        None
                    } else {
                        Some(r.message.content)
                    },
                    tool_calls: r.message.tool_calls,
                    thinking: r.message.thinking,
                },
                done: r.done,
                usage: if r.done {
                    r.final_data.as_ref().map(|f| Usage {
                        input_tokens: f.prompt_eval_count as u32,
                        output_tokens: f.eval_count as u32,
                    })
                } else {
                    None
                },
            }),
            Err(_) => Err("stream error".into()),
        });
        Ok(Box::pin(mapped))
    }
}
