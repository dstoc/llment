use std::error::Error;

use async_trait::async_trait;
use gemini_rs::{
    Client,
    types::{
        Content, FunctionCall, FunctionCallingConfig, FunctionCallingMode, FunctionDeclaration,
        FunctionResponse, GenerationConfig, Part, Role, ThinkingConfig, ToolConfig, Tools,
    },
};
use serde_json::json;
use tokio_stream::StreamExt;

use super::{
    ChatMessageRequest, ChatStream, LlmClient, MessageRole, ResponseChunk, ResponseMessage,
    ToolCall, ToolCallFunction, to_openapi_schema,
};

pub struct GeminiClient {
    inner: Client,
}

impl GeminiClient {
    pub fn new(_host: &str) -> Self {
        Self {
            inner: Client::instance(),
        }
    }
}

#[async_trait]
impl LlmClient for GeminiClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let mut route = self.inner.stream_generate_content(&request.model_name);

        let mut contents: Vec<Content> = Vec::new();
        let mut system_instruction: Option<String> = None;
        for m in request.messages {
            match m.role {
                MessageRole::User => contents.push(Content {
                    role: Role::User,
                    parts: vec![Part::text(&m.content)],
                }),
                MessageRole::Assistant => {
                    if !m.tool_calls.is_empty() {
                        let parts = m
                            .tool_calls
                            .iter()
                            .map(|tool_call| Part {
                                function_call: Some(FunctionCall {
                                    id: None,
                                    name: tool_call.function.name.clone(),
                                    args: tool_call.function.arguments.clone(),
                                }),
                                ..Default::default()
                            })
                            .collect();
                        contents.push(Content {
                            role: Role::Model,
                            parts,
                        });
                    } else {
                        contents.push(Content {
                            role: Role::Model,
                            parts: vec![Part::text(&m.content)],
                        });
                    }
                }
                MessageRole::System => {
                    // TODO: this is weird
                    if let Some(si) = system_instruction.as_mut() {
                        si.push_str("\n");
                        si.push_str(&m.content);
                    } else {
                        system_instruction = Some(m.content);
                    }
                }
                MessageRole::Tool => contents.push(Content {
                    role: Role::Function,
                    parts: vec![Part {
                        function_response: Some(FunctionResponse {
                            id: None,
                            name: m.tool_name.unwrap_or("function".into()),
                            // TODO: try parse m.content as JSON first.
                            response: json!({"content": m.content}),
                        }),
                        ..Default::default()
                    }],
                }),
            }
        }
        route.contents(contents);
        if let Some(si) = system_instruction {
            route.system_instruction(&si);
        }

        if !request.tools.is_empty() {
            let function_declarations: Vec<FunctionDeclaration> = request
                .tools
                .iter()
                .map(|t| FunctionDeclaration {
                    name: t.function.name.clone(),
                    description: t.function.description.clone(),
                    parameters: to_openapi_schema(&t.function.parameters),
                })
                .collect();

            route.tools(vec![Tools {
                function_declarations: Some(function_declarations.clone()),
                google_search: None,
                code_execution: None,
            }]);
            route.tool_config(ToolConfig {
                function_calling_config: Some(FunctionCallingConfig {
                    mode: Some(FunctionCallingMode::Auto),
                    allowed_function_names: None,
                }),
            });
        }

        route.config(GenerationConfig {
            thinking_config: Some(ThinkingConfig {
                thinking_budget: None,
                include_thoughts: Some(true),
            }),
            ..Default::default()
        });

        let stream = route
            .stream()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let mapped = stream.filter_map(move |res| {
            match res {
                Ok(chunk) => {
                    let mut content = String::new();
                    let mut tool_calls = Vec::new();
                    let mut thinking: Option<String> = None;

                    if let Some(candidate) = chunk.candidates.get(0) {
                        for part in &candidate.content.parts {
                            if let Some(fc) = &part.function_call {
                                tool_calls.push(ToolCall {
                                    function: ToolCallFunction {
                                        name: fc.name.clone(),
                                        arguments: fc.args.clone(),
                                    },
                                });
                            } else if let Some(text) = &part.text {
                                if part.thought == Some(true) {
                                    thinking.get_or_insert_with(String::new).push_str(text);
                                } else {
                                    content.push_str(text);
                                }
                            }
                        }
                    }

                    let is_empty =
                        content.is_empty() && tool_calls.is_empty() && thinking.is_none();
                    let done = chunk.candidates.iter().any(|c| c.finish_reason.is_some());

                    if done || !is_empty {
                        Some(Ok(ResponseChunk {
                            message: ResponseMessage {
                                content,
                                tool_calls,
                                thinking,
                            },
                            done,
                        }))
                    } else {
                        None
                    }
                }
                Err(e) => Some(Err(e.into())), // preserve errors
            }
        });
        Ok(Box::pin(mapped))
    }
}
