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
    ChatMessage, ChatMessageRequest, ChatStream, LlmClient, ResponseChunk, ResponseMessage,
    ToolCall, Usage, to_openapi_schema,
};

pub struct GeminiClient {
    inner: Client,
}

impl GeminiClient {
    pub fn new(_host: Option<&str>) -> Self {
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
            match m {
                ChatMessage::User(u) => contents.push(Content {
                    role: Role::User,
                    parts: vec![Part::text(&u.content)],
                }),
                ChatMessage::Assistant(a) => {
                    if !a.tool_calls.is_empty() {
                        let parts = a
                            .tool_calls
                            .iter()
                            .map(|tool_call| Part {
                                function_call: Some(FunctionCall {
                                    id: None,
                                    name: tool_call.name.clone(),
                                    args: tool_call.arguments.clone(),
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
                            parts: vec![Part::text(&a.content)],
                        });
                    }
                }
                ChatMessage::System(s) => {
                    // TODO: this is weird
                    if let Some(si) = system_instruction.as_mut() {
                        si.push_str("\n");
                        si.push_str(&s.content);
                    } else {
                        system_instruction = Some(s.content);
                    }
                }
                ChatMessage::Tool(t) => contents.push(Content {
                    role: Role::Function,
                    parts: vec![Part {
                        function_response: Some(FunctionResponse {
                            id: None,
                            name: t.tool_name,
                            // TODO: try parse t.content as JSON first.
                            response: json!({"content": t.content}),
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
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: to_openapi_schema(&t.parameters),
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
                    let mut content_acc = String::new();
                    let mut tool_calls = Vec::new();
                    let mut thinking: Option<String> = None;

                    if let Some(candidate) = chunk.candidates.get(0) {
                        for part in &candidate.content.parts {
                            if let Some(fc) = &part.function_call {
                                tool_calls.push(ToolCall {
                                    name: fc.name.clone(),
                                    arguments: fc.args.clone(),
                                });
                            } else if let Some(text) = &part.text {
                                if part.thought == Some(true) {
                                    thinking.get_or_insert_with(String::new).push_str(text);
                                } else {
                                    content_acc.push_str(text);
                                }
                            }
                        }
                    }

                    let content = if content_acc.is_empty() {
                        None
                    } else {
                        Some(content_acc)
                    };
                    let is_empty = content.is_none() && tool_calls.is_empty() && thinking.is_none();
                    let done = chunk.candidates.iter().any(|c| c.finish_reason.is_some());
                    let usage = if done {
                        chunk.usage_metadata.as_ref().map(|u| Usage {
                            input_tokens: u.prompt_token_count as u32,
                            output_tokens: u.candidates_token_count.unwrap_or(0) as u32,
                        })
                    } else {
                        None
                    };

                    if done || !is_empty {
                        Some(Ok(ResponseChunk {
                            message: ResponseMessage {
                                content,
                                tool_calls,
                                thinking,
                            },
                            done,
                            usage,
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

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let resp = self.inner.models().await?;
        Ok(resp.models.into_iter().map(|m| m.name).collect())
    }
}
