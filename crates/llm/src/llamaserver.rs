use std::error::Error;

use super::{
    ChatMessage, ChatMessageRequest, ChatStream, LlmClient, ResponseChunk, ResponseMessage,
    ToolCall, Usage as LlmUsage, to_openapi_schema,
};
use async_openai::{Client, config::OpenAIConfig, types::*};
use async_trait::async_trait;
use openai_harmony::{
    HarmonyEncodingName, StreamableParser,
    chat::{
        Author, Content, Conversation, DeveloperContent, Message, Role, SystemContent, TextContent,
        ToolDescription,
    },
    load_harmony_encoding,
};
use serde_json::Value;
use tokio_stream::StreamExt;
use uuid::Uuid;

pub struct LlamaServerClient {
    inner: Client<OpenAIConfig>,
}

impl LlamaServerClient {
    pub fn new(host: Option<&str>) -> Self {
        let config = match host {
            Some(h) => OpenAIConfig::default().with_api_base(h),
            None => OpenAIConfig::default().with_api_base("http://localhost:8000/v1"),
        };
        Self {
            inner: Client::with_config(config),
        }
    }
}

#[async_trait]
impl LlmClient for LlamaServerClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let encoding = tokio::task::spawn_blocking(|| {
            load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss)
        })
        .await
        .map_err(|e| Box::<dyn Error + Send + Sync>::from(e))?
        .map_err(|e| Box::<dyn Error + Send + Sync>::from(e))?;
        let mut system_msgs = Vec::new();
        let mut other_msgs = Vec::new();
        let mut developer = DeveloperContent::new();
        for msg in request.messages {
            match msg {
                ChatMessage::System(s) => {
                    if !s.content.is_empty() {
                        developer = developer.with_instructions(s.content);
                    }
                }
                other => other_msgs.push(other),
            }
        }
        if !request.tools.is_empty() {
            let tools: Vec<ToolDescription> = request
                .tools
                .into_iter()
                .map(|t| {
                    ToolDescription::new(
                        t.name,
                        t.description,
                        Some(to_openapi_schema(&t.parameters)),
                    )
                })
                .collect();
            developer = developer.with_function_tools(tools);
        }
        system_msgs.push(Message::from_role_and_content(
            Role::System,
            SystemContent::new(),
        ));
        if developer.instructions.is_some() || developer.tools.is_some() {
            system_msgs.push(Message::from_role_and_content(Role::Developer, developer));
        }
        let mut convo_msgs = system_msgs;
        for msg in other_msgs {
            match msg {
                ChatMessage::User(u) => {
                    convo_msgs.push(Message::from_role_and_content(Role::User, u.content));
                }
                ChatMessage::Assistant(a) => {
                    if let Some(thinking) = a.thinking {
                        if !thinking.is_empty() {
                            convo_msgs.push(
                                Message::from_role_and_content(Role::Assistant, thinking)
                                    .with_channel("analysis"),
                            );
                        }
                    }
                    for tc in a.tool_calls {
                        let args = tc.arguments.to_string();
                        convo_msgs.push(
                            Message::from_role_and_content(Role::Assistant, args)
                                .with_channel("commentary")
                                .with_recipient(format!("functions.{}", tc.name))
                                .with_content_type("<|constrain|>json"),
                        );
                    }
                    if !a.content.is_empty() {
                        convo_msgs.push(
                            Message::from_role_and_content(Role::Assistant, a.content)
                                .with_channel("final"),
                        );
                    }
                }
                ChatMessage::Tool(t) => {
                    let content_str = match t.content {
                        Value::String(s) => s,
                        v => v.to_string(),
                    };
                    convo_msgs.push(Message::from_author_and_content(
                        Author::new(Role::Tool, format!("functions.{}", t.tool_name)),
                        content_str,
                    ));
                }
                ChatMessage::System(_) => {}
            }
        }
        let conversation = Conversation::from_messages(convo_msgs);
        let tokens =
            encoding.render_conversation_for_completion(&conversation, Role::Assistant, None)?;
        let prompt = encoding.tokenizer().decode_utf8(&tokens)?;
        let req = CreateCompletionRequestArgs::default()
            .model(request.model_name)
            .prompt(prompt)
            .stream(true)
            .stream_options(ChatCompletionStreamOptions {
                include_usage: true,
            })
            .build()?;
        let stream = self.inner.completions().create_stream(req).await?;
        let mut parser = StreamableParser::new(encoding.clone(), Some(Role::Assistant))?;
        let mut seen = 0usize;
        let mapped = stream.map(move |res| {
            res.map(|chunk| {
                let mut msg = ResponseMessage {
                    content: None,
                    tool_calls: Vec::new(),
                    thinking: None,
                };
                if let Some(choice) = chunk.choices.first() {
                    if !choice.text.is_empty() {
                        let tokens = encoding
                            .tokenizer()
                            .encode_with_special_tokens(&choice.text);
                        for t in tokens {
                            parser.process(t).ok();
                        }
                        if let Some(delta) = parser.last_content_delta().ok().flatten() {
                            match parser.current_channel().as_deref() {
                                Some("analysis") => msg.thinking = Some(delta),
                                Some("final") => msg.content = Some(delta),
                                _ => {}
                            }
                        }
                    }
                }
                let messages = parser.messages();
                while seen < messages.len() {
                    if let Some(recipient) = &messages[seen].recipient {
                        if let Some(name) = recipient.strip_prefix("functions.") {
                            if let Some(Content::Text(TextContent { text })) =
                                messages[seen].content.first()
                            {
                                let args: Value = serde_json::from_str(text).unwrap_or(Value::Null);
                                msg.tool_calls.push(ToolCall {
                                    id: Uuid::new_v4().to_string(),
                                    name: name.to_string(),
                                    arguments: args,
                                });
                            }
                        }
                    }
                    seen += 1;
                }
                let mut done = false;
                let mut usage = None;
                if let Some(u) = chunk.usage {
                    parser.process_eos().ok();
                    let messages = parser.messages();
                    while seen < messages.len() {
                        if let Some(recipient) = &messages[seen].recipient {
                            if let Some(name) = recipient.strip_prefix("functions.") {
                                if let Some(Content::Text(TextContent { text })) =
                                    messages[seen].content.first()
                                {
                                    let args: Value =
                                        serde_json::from_str(text).unwrap_or(Value::Null);
                                    msg.tool_calls.push(ToolCall {
                                        id: Uuid::new_v4().to_string(),
                                        name: name.to_string(),
                                        arguments: args,
                                    });
                                }
                            }
                        }
                        seen += 1;
                    }
                    usage = Some(LlmUsage {
                        input_tokens: u.prompt_tokens,
                        output_tokens: u.completion_tokens,
                    });
                    done = true;
                }
                ResponseChunk {
                    message: msg,
                    done,
                    usage,
                }
            })
            .map_err(|e| e.into())
        });
        Ok(Box::pin(mapped))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Ok(vec!["gpt-oss".to_string()])
    }
}
