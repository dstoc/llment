use std::error::Error;

use async_trait::async_trait;
use futures_util::StreamExt;
use gemini_rust::{
    Content, FunctionCallingMode, FunctionDeclaration, FunctionParameters, Gemini, Message, Part,
    Role,
};
use reqwest::Client as HttpClient;
use uuid::Uuid;

use super::{
    AssistantPart, ChatMessage, ChatMessageRequest, ChatStream, LlmClient, ResponseChunk, ToolCall,
    to_openapi_schema,
};

pub struct GeminiRustClient {
    api_key: String,
    base_url: String,
    http_client: HttpClient,
}

impl GeminiRustClient {
    pub fn new(host: Option<&str>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "GEMINI_API_KEY not set")
        })?;
        let base_url = host
            .map(|h| {
                let mut s = h.to_string();
                if !s.ends_with('/') {
                    s.push('/');
                }
                s
            })
            .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta/".to_string());
        Ok(Self {
            api_key,
            base_url,
            http_client: HttpClient::new(),
        })
    }
}

#[async_trait]
impl LlmClient for GeminiRustClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let gemini = Gemini::with_model_and_base_url(
            self.api_key.clone(),
            format!("models/{}", request.model_name),
            self.base_url.clone(),
        );
        let mut builder = gemini.generate_content();

        let mut system_instruction: Option<String> = None;
        for m in request.messages {
            match m {
                ChatMessage::User(u) => {
                    builder = builder.with_user_message(u.content);
                }
                ChatMessage::Assistant(a) => {
                    let mut parts_vec: Vec<Part> = Vec::new();
                    for part in a.content {
                        match part {
                            AssistantPart::Text { text } => {
                                parts_vec.push(Part::Text {
                                    text,
                                    thought: None,
                                });
                            }
                            AssistantPart::ToolCall(tc) => {
                                parts_vec.push(Part::FunctionCall {
                                    function_call: gemini_rust::FunctionCall::new(
                                        tc.name,
                                        tc.arguments,
                                    ),
                                });
                            }
                            AssistantPart::Thinking { .. } => {}
                        }
                    }
                    if !parts_vec.is_empty() {
                        let content = Content {
                            parts: Some(parts_vec),
                            role: Some(Role::Model),
                        };
                        builder = builder.with_message(Message {
                            content,
                            role: Role::Model,
                        });
                    }
                }
                ChatMessage::System(s) => {
                    if let Some(si) = system_instruction.as_mut() {
                        si.push_str("\n");
                        si.push_str(&s.content);
                    } else {
                        system_instruction = Some(s.content);
                    }
                }
                ChatMessage::Tool(t) => {
                    builder = builder.with_function_response(
                        t.tool_name,
                        serde_json::json!({ "output": t.content }),
                    );
                    // TODO: Support the "error" field on tool responses.
                }
            }
        }
        if let Some(si) = system_instruction {
            builder = builder.with_system_instruction(si);
        }

        if !request.tools.is_empty() {
            for t in request.tools {
                let params_value = to_openapi_schema(&t.parameters);
                let params: FunctionParameters = serde_json::from_value(params_value)?;
                let function = FunctionDeclaration::new(t.name, t.description, params);
                builder = builder.with_function(function);
            }
            builder = builder.with_function_calling_mode(FunctionCallingMode::Auto);
        }

        if request.think.unwrap_or(true) {
            builder = builder.with_thinking_config(gemini_rust::ThinkingConfig {
                thinking_budget: Some(-1),
                include_thoughts: Some(true),
            });
        }

        let mut input_tokens = 0u32;
        let mut output_tokens = 0u32;
        let stream = builder.execute_stream().await?;
        let mapped = stream.flat_map(move |res| match res {
            Ok(chunk) => {
                let mut out: Vec<Result<ResponseChunk, Box<dyn Error + Send + Sync>>> = Vec::new();
                if let Some(usage) = chunk.usage_metadata {
                    let input_delta = usage.prompt_token_count as u32 - input_tokens;
                    input_tokens += input_delta;
                    let output_delta = usage.total_token_count as u32
                        - usage.prompt_token_count as u32
                        - output_tokens;
                    output_tokens += output_delta;
                    out.push(Ok(ResponseChunk::Usage {
                        input_tokens: input_delta,
                        output_tokens: output_delta,
                    }));
                }
                if let Some(candidate) = chunk.candidates.first() {
                    if let Some(parts) = &candidate.content.parts {
                        for part in parts {
                            match part {
                                Part::Text { text, thought } => {
                                    if thought.unwrap_or(false) {
                                        out.push(Ok(ResponseChunk::Thinking(text.clone())));
                                    } else if !text.is_empty() {
                                        out.push(Ok(ResponseChunk::Content(text.clone())));
                                    }
                                }
                                Part::FunctionCall { function_call } => {
                                    out.push(Ok(ResponseChunk::ToolCall(ToolCall {
                                        id: Uuid::new_v4().to_string(),
                                        name: function_call.name.clone(),
                                        arguments: function_call.args.clone(),
                                        arguments_invalid: None,
                                    })));
                                }
                                _ => {}
                            }
                        }
                    }
                    if candidate.finish_reason.is_some() {
                        out.push(Ok(ResponseChunk::Done));
                    }
                }
                tokio_stream::iter(out)
            }
            Err(e) => tokio_stream::iter(vec![Err::<ResponseChunk, _>(e.into())]),
        });
        Ok(Box::pin(mapped))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let url = format!("{}models?key={}", self.base_url, self.api_key);
        let resp = self.http_client.get(url).send().await?;
        let value: serde_json::Value = resp.json().await?;
        let models = value["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(models)
    }
}
