use std::error::Error;

use super::{
    ChatMessageRequest, ChatStream, LlmClient, MessageRole, ResponseChunk, ResponseMessage,
    ToolCall, ToolCallFunction, Usage as LlmUsage, to_openapi_schema,
};
use async_openai::{Client, config::OpenAIConfig, types::*};
use async_trait::async_trait;
use serde_json::Value;
use tokio_stream::StreamExt;

pub struct OpenAiClient {
    inner: Client<OpenAIConfig>,
}

impl OpenAiClient {
    pub fn new(host: Option<&str>) -> Self {
        let config = match host {
            Some(h) => OpenAIConfig::default().with_api_base(h),
            None => OpenAIConfig::default(),
        };
        Self {
            inner: Client::with_config(config),
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let messages: Vec<ChatCompletionRequestMessage> = request
            .messages
            .into_iter()
            .map(|m| match m.role {
                MessageRole::User => ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content(ChatCompletionRequestUserMessageContent::Text(m.content))
                        .build()
                        .unwrap(),
                ),
                MessageRole::Assistant => ChatCompletionRequestMessage::Assistant(
                    ChatCompletionRequestAssistantMessageArgs::default()
                        .content(ChatCompletionRequestAssistantMessageContent::Text(
                            m.content,
                        ))
                        .build()
                        .unwrap(),
                ),
                MessageRole::System => ChatCompletionRequestMessage::System(
                    ChatCompletionRequestSystemMessageArgs::default()
                        .content(ChatCompletionRequestSystemMessageContent::Text(m.content))
                        .build()
                        .unwrap(),
                ),
                MessageRole::Tool => ChatCompletionRequestMessage::Tool(
                    ChatCompletionRequestToolMessageArgs::default()
                        .content(ChatCompletionRequestToolMessageContent::Text(m.content))
                        .tool_call_id(m.tool_name.unwrap_or_default())
                        .build()
                        .unwrap(),
                ),
            })
            .collect();

        let tools: Option<Vec<ChatCompletionTool>> = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .into_iter()
                    .map(|t| {
                        ChatCompletionToolArgs::default()
                            .function(FunctionObject {
                                name: t.function.name,
                                description: Some(t.function.description),
                                parameters: Some(to_openapi_schema(&t.function.parameters)),
                                strict: None,
                            })
                            .build()
                            .unwrap()
                    })
                    .collect(),
            )
        };

        let mut req_builder = CreateChatCompletionRequestArgs::default();
        req_builder.model(request.model_name);
        req_builder.messages(messages);
        if let Some(t) = tools {
            req_builder.tools(t);
        }
        req_builder.stream_options(ChatCompletionStreamOptions {
            include_usage: true,
        });
        let req = req_builder.build()?;
        let stream = self.inner.chat().create_stream(req).await?;
        let mapped = stream.map(|res| {
            res.map(|chunk| {
                let mut content_acc = String::new();
                let mut tool_calls = Vec::new();
                for choice in &chunk.choices {
                    if let Some(c) = choice.delta.content.as_deref() {
                        content_acc.push_str(c);
                    }
                    if let Some(calls) = &choice.delta.tool_calls {
                        for tc in calls {
                            if let Some(func) = &tc.function {
                                let args: Value = func
                                    .arguments
                                    .as_deref()
                                    .and_then(|a| serde_json::from_str(a).ok())
                                    .unwrap_or(Value::Null);
                                tool_calls.push(ToolCall {
                                    function: ToolCallFunction {
                                        name: func.name.clone().unwrap_or_default(),
                                        arguments: args,
                                    },
                                });
                            }
                        }
                    }
                }
                let content = if content_acc.is_empty() {
                    None
                } else {
                    Some(content_acc)
                };
                let done = chunk.choices.iter().any(|c| c.finish_reason.is_some());
                let usage = if done {
                    chunk.usage.map(|u| LlmUsage {
                        input_tokens: u.prompt_tokens as u32,
                        output_tokens: u.completion_tokens as u32,
                    })
                } else {
                    None
                };
                ResponseChunk {
                    message: ResponseMessage {
                        content,
                        tool_calls,
                        thinking: None,
                    },
                    done,
                    usage,
                }
            })
            .map_err(|e| e.into())
        });
        Ok(Box::pin(mapped))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let resp = self.inner.models().list().await?;
        Ok(resp.data.into_iter().map(|m| m.id).collect())
    }
}
