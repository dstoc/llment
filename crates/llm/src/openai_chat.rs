use std::error::Error;

use super::{
    AssistantPart, ChatMessage, ChatMessageRequest, ChatStream, JsonResult, LlmClient,
    ResponseChunk, ToolCall, to_openapi_schema,
};
use async_openai::{Client, config::OpenAIConfig, types::*};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Default)]
struct ToolCallBuilder {
    id: Option<String>,
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

pub struct OpenAiChatClient {
    inner: Client<OpenAIConfig>,
}

impl OpenAiChatClient {
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
impl LlmClient for OpenAiChatClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let messages: Vec<Value> = request
            .messages
            .into_iter()
            .map(|m| match m {
                ChatMessage::User(u) => serde_json::to_value(ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content(ChatCompletionRequestUserMessageContent::Text(u.content))
                        .build()
                        .unwrap(),
                )),
                ChatMessage::Assistant(a) => {
                    let mut builder = ChatCompletionRequestAssistantMessageArgs::default();
                    let mut content_acc = String::new();
                    let mut thinking_acc = String::new();
                    let mut tool_calls_acc: Vec<ToolCall> = Vec::new();
                    for part in a.content {
                        match part {
                            AssistantPart::Text { text, .. } => content_acc.push_str(&text),
                            AssistantPart::Thinking { text, .. } => thinking_acc.push_str(&text),
                            AssistantPart::ToolCall { call, .. } => tool_calls_acc.push(call),
                        }
                    }
                    if !content_acc.is_empty() {
                        builder.content(ChatCompletionRequestAssistantMessageContent::Text(
                            content_acc,
                        ));
                    }
                    if !tool_calls_acc.is_empty() {
                        let tool_calls: Vec<ChatCompletionMessageToolCall> = tool_calls_acc
                            .into_iter()
                            .map(|tc| {
                                let args = match &tc.arguments {
                                    JsonResult::Content { .. } => {
                                        tc.arguments_content_with_id().to_string()
                                    }
                                    JsonResult::Error { error } => error.clone(),
                                };
                                ChatCompletionMessageToolCall {
                                    id: tc.id,
                                    r#type: ChatCompletionToolType::Function,
                                    function: FunctionCall {
                                        name: tc.name,
                                        arguments: args,
                                    },
                                }
                            })
                            .collect();
                        builder.tool_calls(tool_calls);
                    }
                    let result = serde_json::to_value(ChatCompletionRequestMessage::Assistant(
                        builder.build().unwrap(),
                    ));
                    if !thinking_acc.is_empty() {
                        result.map(|mut inner| {
                            inner.as_object_mut().unwrap().insert(
                                "reasoning_content".to_string(),
                                Value::String(thinking_acc),
                            );
                            inner
                        })
                    } else {
                        result
                    }
                }
                ChatMessage::System(s) => {
                    serde_json::to_value(ChatCompletionRequestMessage::System(
                        ChatCompletionRequestSystemMessageArgs::default()
                            .content(ChatCompletionRequestSystemMessageContent::Text(s.content))
                            .build()
                            .unwrap(),
                    ))
                }
                ChatMessage::Tool(t) => {
                    let content_str = match &t.content {
                        JsonResult::Content { content } => match content {
                            Value::String(s) => s.clone(),
                            v => v.to_string(),
                        },
                        JsonResult::Error { error } => error.clone(),
                    };
                    serde_json::to_value(ChatCompletionRequestMessage::Tool(
                        ChatCompletionRequestToolMessageArgs::default()
                            .content(ChatCompletionRequestToolMessageContent::Text(content_str))
                            .tool_call_id(t.id)
                            .build()
                            .unwrap(),
                    ))
                }
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
                                name: t.name,
                                description: Some(t.description),
                                parameters: Some(to_openapi_schema(&t.parameters)),
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
        let mapped = stream.flat_map(move |res| {
            let mut out: Vec<Result<ResponseChunk, Box<dyn Error + Send + Sync>>> = Vec::new();
            match res {
                Ok(chunk) => {
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
                                    pending_tool_calls
                                        .resize_with(index + 1, ToolCallBuilder::default);
                                }
                                if let Some(id) = &tc.id {
                                    pending_tool_calls[index].id = Some(id.clone());
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
                                    let arguments = match serde_json::from_str(&b.arguments) {
                                        Ok(v) => JsonResult::Content { content: v },
                                        Err(_) => JsonResult::Error {
                                            error: b.arguments.clone(),
                                        },
                                    };
                                    tool_calls.push(ToolCall {
                                        id: b.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                                        name: b.name.unwrap_or_default(),
                                        arguments,
                                    });
                                }
                            }
                        }
                    }
                    let done = chunk.choices.iter().any(|c| c.finish_reason.is_some());
                    let usage = if done {
                        chunk
                            .usage
                            .map(|u| (u.prompt_tokens as u32, u.completion_tokens as u32))
                    } else {
                        None
                    };
                    if !thinking_acc.is_empty() {
                        out.push(Ok(ResponseChunk::Part(AssistantPart::Thinking {
                            text: thinking_acc,
                            encrypted_content: None,
                        })));
                    }
                    for tc in tool_calls {
                        out.push(Ok(ResponseChunk::Part(AssistantPart::ToolCall {
                            call: tc,
                            encrypted_content: None,
                        })));
                    }
                    if !content_acc.is_empty() {
                        out.push(Ok(ResponseChunk::Part(AssistantPart::Text {
                            text: content_acc,
                            encrypted_content: None,
                        })));
                    }
                    if let Some((input_tokens, output_tokens)) = usage {
                        out.push(Ok(ResponseChunk::Usage {
                            input_tokens,
                            output_tokens,
                        }));
                    }
                    if done {
                        out.push(Ok(ResponseChunk::Done));
                    }
                }
                Err(e) => out.push(Err(e.into())),
            }
            tokio_stream::iter(out)
        });
        Ok(Box::pin(mapped))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let resp = self.inner.models().list().await?;
        Ok(resp.data.into_iter().map(|m| m.id).collect())
    }
}
