use std::error::Error;

use async_trait::async_trait;
use futures_util::StreamExt;
use ollama_rs::{
    Ollama,
    generation::{
        chat::{
            ChatMessage as OllamaChatMessage, ChatMessageResponseStream,
            MessageRole as OllamaMessageRole,
            request::ChatMessageRequest as OllamaChatMessageRequest,
        },
        tools::{
            ToolCall as OllamaToolCall, ToolCallFunction as OllamaToolCallFunction,
            ToolFunctionInfo as OllamaToolFunctionInfo, ToolInfo as OllamaToolInfo,
            ToolType as OllamaToolType,
        },
    },
};
use serde_json::Value;
use uuid::Uuid;

use super::{
    AssistantPart, ChatMessage, ChatMessageRequest, ChatStream, JsonResult, LlmClient,
    ResponseChunk, ToolCall,
};

pub struct OllamaClient {
    inner: Ollama,
}

impl OllamaClient {
    pub fn new(host: Option<&str>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let host = host.unwrap_or("http://127.0.0.1:11434");
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
        let ollama_request = {
            let messages = request
                .messages
                .into_iter()
                .map(|m| match m {
                    ChatMessage::User(u) => {
                        OllamaChatMessage::new(OllamaMessageRole::User, u.content)
                    }
                    ChatMessage::Assistant(a) => {
                        let mut msg =
                            OllamaChatMessage::new(OllamaMessageRole::Assistant, String::new());
                        for part in a.content {
                            match part {
                                AssistantPart::Text { text } => {
                                    msg.content.push_str(&text);
                                }
                                AssistantPart::ToolCall(tc) => {
                                    msg.tool_calls.push(OllamaToolCall {
                                        function: OllamaToolCallFunction {
                                            name: tc.name,
                                            arguments: tc.arguments,
                                        },
                                    });
                                }
                                AssistantPart::Thinking { text } => {
                                    let thinking = msg.thinking.get_or_insert_with(String::new);
                                    thinking.push_str(&text);
                                }
                            }
                        }
                        msg
                    }
                    ChatMessage::System(s) => {
                        OllamaChatMessage::new(OllamaMessageRole::System, s.content)
                    }
                    ChatMessage::Tool(t) => {
                        let content_str = match t.content {
                            JsonResult::Content { content } => match content {
                                Value::String(s) => s,
                                v => v.to_string(),
                            },
                            JsonResult::Error { error } => error,
                        };
                        let mut msg = OllamaChatMessage::new(OllamaMessageRole::Tool, content_str);
                        msg.tool_name = Some(t.tool_name);
                        msg
                    }
                })
                .collect();

            let tools = request
                .tools
                .into_iter()
                .map(|t| OllamaToolInfo {
                    tool_type: OllamaToolType::Function,
                    function: OllamaToolFunctionInfo {
                        name: t.name,
                        description: t.description,
                        parameters: t.parameters,
                    },
                })
                .collect();

            let mut req = OllamaChatMessageRequest::new(request.model_name, messages).tools(tools);
            if let Some(t) = request.think {
                req = req.think(t);
            }
            req
        };
        let stream: ChatMessageResponseStream =
            self.inner.send_chat_messages_stream(ollama_request).await?;
        let mapped = stream.flat_map(|res| match res {
            Ok(r) => {
                let mut out: Vec<Result<ResponseChunk, Box<dyn Error + Send + Sync>>> = Vec::new();
                if !r.message.thinking.clone().unwrap_or_default().is_empty() {
                    if let Some(thinking) = r.message.thinking.clone() {
                        out.push(Ok(ResponseChunk::Thinking(thinking)));
                    }
                }
                let tool_calls: Vec<ToolCall> = r
                    .message
                    .tool_calls
                    .into_iter()
                    .map(|tc| ToolCall {
                        id: Uuid::new_v4().to_string(),
                        name: tc.function.name,
                        arguments: tc.function.arguments,
                        arguments_invalid: None,
                    })
                    .collect();
                for tc in tool_calls {
                    out.push(Ok(ResponseChunk::ToolCall(tc)));
                }
                if !r.message.content.is_empty() {
                    out.push(Ok(ResponseChunk::Content(r.message.content)));
                }
                if r.done {
                    if let Some(f) = r.final_data.as_ref() {
                        out.push(Ok(ResponseChunk::Usage {
                            input_tokens: f.prompt_eval_count as u32,
                            output_tokens: f.eval_count as u32,
                        }));
                    }
                    out.push(Ok(ResponseChunk::Done));
                }
                tokio_stream::iter(out)
            }
            Err(_) => tokio_stream::iter(vec![Err::<ResponseChunk, _>("stream error".into())]),
        });
        Ok(Box::pin(mapped))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let models = self.inner.list_local_models().await?;
        Ok(models.into_iter().map(|m| m.name).collect())
    }
}
