use std::{error::Error, rc::Rc};

use super::{
    ChatMessageRequest, ChatStream, LlmClient, MessageRole, ResponseChunk, ResponseMessage,
    ToolCall, ToolCallFunction, Usage as LlmUsage, to_openapi_schema,
};
use async_openai::{Client, config::OpenAIConfig, types::*};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio_stream::StreamExt;

#[derive(Default)]
struct ToolCallBuilder {
    name: Option<String>,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct StreamingChunk {
    choices: Vec<StreamingChoice>,
    usage: Option<CompletionUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamingChoice {
    delta: StreamingDelta,
    finish_reason: Option<FinishReason>,
}

#[derive(Debug, Deserialize)]
struct StreamingDelta {
    content: Option<String>,
    #[serde(rename = "reasoning_content")]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ChatCompletionMessageToolCallChunk>>,
}

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
        let messages: Vec<Value> = request
            .messages
            .into_iter()
            .map(|m| match m.role {
                MessageRole::User => serde_json::to_value(ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content(ChatCompletionRequestUserMessageContent::Text(m.content))
                        .build()
                        .unwrap(),
                )),
                MessageRole::Assistant => {
                    let mut builder = ChatCompletionRequestAssistantMessageArgs::default();
                    if !m.content.is_empty() {
                        builder.content(ChatCompletionRequestAssistantMessageContent::Text(
                            m.content,
                        ));
                    }
                    if !m.tool_calls.is_empty() {
                        let tool_calls: Vec<ChatCompletionMessageToolCall> = m
                            .tool_calls
                            .into_iter()
                            .map(|tc| ChatCompletionMessageToolCall {
                                id: tc.function.name.clone(),
                                r#type: ChatCompletionToolType::Function,
                                function: FunctionCall {
                                    name: tc.function.name,
                                    arguments: tc.function.arguments.to_string(),
                                },
                            })
                            .collect();
                        builder.tool_calls(tool_calls);
                    }
                    let result = serde_json::to_value(ChatCompletionRequestMessage::Assistant(
                        builder.build().unwrap(),
                    ));
                    if let Some(thinking) = m.thinking {
                        result.map(|mut inner| {
                            inner
                                .as_object_mut()
                                .unwrap()
                                .insert("reasoning_content".to_string(), Value::String(thinking));
                            inner
                        })
                    } else {
                        result
                    }
                }
                MessageRole::System => serde_json::to_value(ChatCompletionRequestMessage::System(
                    ChatCompletionRequestSystemMessageArgs::default()
                        .content(ChatCompletionRequestSystemMessageContent::Text(m.content))
                        .build()
                        .unwrap(),
                )),
                MessageRole::Tool => serde_json::to_value(ChatCompletionRequestMessage::Tool(
                    ChatCompletionRequestToolMessageArgs::default()
                        .content(ChatCompletionRequestToolMessageContent::Text(m.content))
                        .tool_call_id(m.tool_name.unwrap_or_default())
                        .build()
                        .unwrap(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;

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
        if let Some(t) = tools {
            req_builder.tools(t);
        }
        req_builder.stream(true);
        req_builder.stream_options(ChatCompletionStreamOptions {
            include_usage: true,
        });
        let req = req_builder.build()?;
        let mut req_value = serde_json::to_value(&req)?;
        req_value
            .as_object_mut()
            .ok_or("req was not object")?
            .insert("messages".to_string(), Value::Array(messages));
        let stream = self
            .inner
            .chat()
            .create_stream_byot::<Value, StreamingChunk>(req_value)
            .await?;
        let mut pending_tool_calls: Vec<ToolCallBuilder> = Vec::new();
        let mapped = stream.map(move |res| {
            res.map(|chunk| {
                let mut content_acc = String::new();
                let mut thinking_acc = String::new();
                let mut tool_calls = Vec::new();
                for choice in &chunk.choices {
                    if let Some(c) = choice.delta.content.as_deref() {
                        content_acc.push_str(c);
                    }
                    if let Some(r) = choice.delta.reasoning_content.as_deref() {
                        thinking_acc.push_str(r);
                    }
                    if let Some(calls) = &choice.delta.tool_calls {
                        for tc in calls {
                            let index = tc.index as usize;
                            if pending_tool_calls.len() <= index {
                                pending_tool_calls.resize_with(index + 1, ToolCallBuilder::default);
                            }
                            if let Some(func) = &tc.function {
                                if let Some(name) = &func.name {
                                    pending_tool_calls[index].name = Some(name.clone());
                                }
                                if let Some(args) = &func.arguments {
                                    pending_tool_calls[index].arguments.push_str(args);
                                }
                            }
                        }
                    }
                    if matches!(choice.finish_reason, Some(FinishReason::ToolCalls)) {
                        if !pending_tool_calls.is_empty() {
                            for b in pending_tool_calls.drain(..) {
                                let args: Value =
                                    serde_json::from_str(&b.arguments).unwrap_or(Value::Null);
                                tool_calls.push(ToolCall {
                                    function: ToolCallFunction {
                                        name: b.name.unwrap_or_default(),
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
                let thinking = if thinking_acc.is_empty() {
                    None
                } else {
                    Some(thinking_acc)
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
                        thinking,
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
