use std::error::Error;

use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        ReasoningEffort,
        responses::{
            CreateResponseArgs, FunctionArgs, Input, InputContent, InputItem, InputMessage,
            OutputContent, ReasoningConfigArgs, Response, Role, ToolDefinition,
        },
    },
};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    AssistantPart, ChatMessage, ChatMessageRequest, ChatStream, JsonResult, LlmClient,
    ResponseChunk, ToolCall, ToolInfo, to_openapi_schema,
};

pub struct OpenAiResponsesClient {
    inner: Client<OpenAIConfig>,
}

impl OpenAiResponsesClient {
    pub fn new(host: Option<&str>) -> Self {
        let config = match host {
            Some(h) => OpenAIConfig::default().with_api_base(h),
            None => OpenAIConfig::default(),
        };
        Self {
            inner: Client::with_config(config),
        }
    }

    fn build_request_input(
        messages: Vec<ChatMessage>,
    ) -> Result<Vec<InputItem>, Box<dyn Error + Send + Sync>> {
        let mut items = Vec::new();
        for message in messages {
            match message {
                ChatMessage::System(system) => {
                    if system.content.is_empty() {
                        continue;
                    }
                    items.push(InputItem::Message(InputMessage {
                        role: Role::Developer,
                        content: InputContent::TextInput(system.content),
                        ..Default::default()
                    }));
                }
                ChatMessage::User(user) => {
                    items.push(InputItem::Message(InputMessage {
                        role: Role::User,
                        content: InputContent::TextInput(user.content),
                        ..Default::default()
                    }));
                }
                ChatMessage::Assistant(assistant) => {
                    for part in assistant.content {
                        match part {
                            AssistantPart::Text { text, .. } => {
                                items.push(InputItem::Message(InputMessage {
                                    role: Role::Assistant,
                                    content: InputContent::TextInput(text),
                                    ..Default::default()
                                }));
                            }
                            AssistantPart::Thinking {
                                encrypted_content, ..
                            } => {
                                if let Some(enc) = encrypted_content {
                                    let mut reasoning = json!({
                                        "type": "reasoning",
                                        "encrypted_content": enc,
                                        // TODO: we don't need to pass the summary, right?
                                        "summary": Vec::<String>::new(),
                                    });
                                    reasoning
                                        .as_object_mut()
                                        .unwrap()
                                        .insert("encrypted_content".into(), Value::String(enc));
                                    items.push(InputItem::Custom(reasoning));
                                }
                            }
                            AssistantPart::ToolCall { call, .. } => {
                                let arguments = match &call.arguments {
                                    JsonResult::Content { .. } => {
                                        call.arguments_content_with_id().to_string()
                                    }
                                    JsonResult::Error { error } => error.clone(),
                                };
                                items.push(InputItem::Custom(json!({
                                    "type": "function_call",
                                    "call_id": call.id,
                                    "name": call.name,
                                    "arguments": arguments,
                                })));
                            }
                        }
                    }
                }
                ChatMessage::Tool(call_response) => {
                    let content_text = match &call_response.content {
                        JsonResult::Content { content } => match content {
                            Value::String(text) => text.clone(),
                            other => other.to_string(),
                        },
                        JsonResult::Error { error } => error.clone(),
                    };
                    items.push(InputItem::Custom(json!({
                        "type": "function_call_output",
                        "call_id": call_response.id,
                        "output": content_text,
                    })));
                }
            }
        }
        Ok(items)
    }

    fn build_tools(
        tools: Vec<ToolInfo>,
    ) -> Result<Vec<ToolDefinition>, Box<dyn Error + Send + Sync>> {
        tools
            .into_iter()
            .map(|tool| {
                Ok(ToolDefinition::Function(
                    FunctionArgs::default()
                        .name(tool.name)
                        .description(tool.description)
                        .parameters(to_openapi_schema(&tool.parameters))
                        .strict(false)
                        .build()?,
                ))
            })
            .collect()
    }

    fn response_chunks(
        response: Response,
    ) -> Result<Vec<ResponseChunk>, Box<dyn Error + Send + Sync>> {
        if let Some(err) = response.error {
            return Err(format!("{}: {}", err.code, err.message).into());
        }

        let mut chunks = Vec::new();
        for item in response.output {
            match item {
                OutputContent::Message(message) => {
                    for content in message.content {
                        match content {
                            async_openai::types::responses::Content::OutputText(text) => {
                                if !text.text.is_empty() {
                                    chunks.push(ResponseChunk::Part(AssistantPart::Text {
                                        text: text.text,
                                        encrypted_content: None,
                                    }));
                                }
                            }
                            async_openai::types::responses::Content::Refusal(refusal) => {
                                chunks.push(ResponseChunk::Part(AssistantPart::Text {
                                    text: refusal.refusal,
                                    encrypted_content: None,
                                }));
                            }
                        }
                    }
                }
                OutputContent::FunctionCall(call) => {
                    let id = if call.call_id.is_empty() {
                        call.id.clone()
                    } else {
                        call.call_id.clone()
                    };
                    let arguments = match serde_json::from_str(&call.arguments) {
                        Ok(value) => JsonResult::Content { content: value },
                        Err(_) => JsonResult::Error {
                            error: call.arguments.clone(),
                        },
                    };
                    chunks.push(ResponseChunk::Part(AssistantPart::ToolCall {
                        call: ToolCall {
                            id,
                            name: call.name,
                            arguments,
                        },
                        encrypted_content: None,
                    }));
                }
                OutputContent::Reasoning(reasoning) => {
                    // TODO: Push separate parts. Use a different field for summary?
                    let text = reasoning
                        .summary
                        .into_iter()
                        .map(|summary| summary.text)
                        .collect::<Vec<_>>()
                        .join("\n");
                    chunks.push(ResponseChunk::Part(AssistantPart::Thinking {
                        text,
                        encrypted_content: reasoning.encrypted_content,
                    }));
                }
                other => {
                    let text = serde_json::to_string(&other).unwrap_or_default();
                    if !text.is_empty() {
                        chunks.push(ResponseChunk::Part(AssistantPart::Text {
                            text,
                            encrypted_content: None,
                        }));
                    }
                }
            }
        }

        if let Some(usage) = response.usage {
            chunks.push(ResponseChunk::Usage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
            });
        }

        chunks.push(ResponseChunk::Done);
        Ok(chunks)
    }
}

#[async_trait]
impl LlmClient for OpenAiResponsesClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let input_items = Self::build_request_input(request.messages)?;
        let tools = Self::build_tools(request.tools)?;

        let mut builder = CreateResponseArgs::default();
        builder
            .model(request.model_name)
            .input(Input::Items(input_items))
            .store(false)
            .reasoning(
                ReasoningConfigArgs::default()
                    // .summary()
                    .effort(ReasoningEffort::Medium)
                    .build()?,
            )
            .include(vec!["reasoning.encrypted_content".to_string()])
            .parallel_tool_calls(true)
            .tools(tools);
        let response = self.inner.responses().create(builder.build()?).await?;
        let chunks = Self::response_chunks(response)?;
        let stream = tokio_stream::iter(chunks.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let resp = self.inner.models().list().await?;
        Ok(resp.data.into_iter().map(|m| m.id).collect())
    }
}
