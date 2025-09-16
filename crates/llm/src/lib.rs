use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use clap::ValueEnum;
use schemars::Schema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, to_value};
use tokio_stream::Stream;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ChatMessage {
    User(UserMessage),
    Assistant(AssistantMessage),
    System(SystemMessage),
    Tool(ToolMessage),
}

impl ChatMessage {
    pub fn user(content: String) -> Self {
        Self::User(UserMessage { content })
    }

    pub fn assistant(content: String) -> Self {
        Self::Assistant(AssistantMessage {
            content: vec![AssistantPart::Text {
                text: content,
                encrypted_content: None,
            }],
        })
    }

    pub fn system(content: String) -> Self {
        Self::System(SystemMessage { content })
    }

    pub fn tool(id: String, content: JsonResult, tool_name: String) -> Self {
        Self::Tool(ToolMessage {
            id,
            tool_name,
            content,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserMessage {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssistantMessage {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<AssistantPart>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemMessage {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolMessage {
    pub id: String,
    pub tool_name: String,
    #[serde(flatten)]
    pub content: JsonResult,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonResult {
    Content { content: Value },
    Error { error: String },
}

impl JsonResult {
    pub fn as_result(&self) -> Result<&Value, &str> {
        match self {
            JsonResult::Content { content } => Ok(content),
            JsonResult::Error { error } => Err(error),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub arguments: JsonResult,
}

impl ToolCall {
    pub fn arguments_content_with_id(&self) -> Value {
        let mut result = (*self.arguments.as_result().unwrap()).clone();
        result
            .as_object_mut()
            .unwrap()
            .insert("_id".into(), Value::String(self.id.clone()));
        result
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantPart {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
    },
    ToolCall {
        #[serde(flatten)]
        call: ToolCall,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
    },
    Thinking {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: Schema,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessageRequest {
    pub model_name: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub think: Option<bool>,
}

impl ChatMessageRequest {
    pub fn new(model_name: String, messages: Vec<ChatMessage>) -> Self {
        Self {
            model_name,
            messages,
            tools: Vec::new(),
            think: None,
        }
    }

    pub fn tools(mut self, tools: Vec<ToolInfo>) -> Self {
        self.tools = tools;
        self
    }

    pub fn think(mut self, think: bool) -> Self {
        self.think = Some(think);
        self
    }
}

pub mod gemini_rust;
pub mod harmony;
mod llama_server;
pub mod mcp;
pub mod ollama;
pub mod openai_chat;
pub mod test_provider;
pub mod tools;

pub use test_provider::TestProvider;

#[derive(Default, Copy, Clone, Debug, ValueEnum)]
pub enum Provider {
    #[default]
    Ollama,
    #[clap(name = "openai-chat")]
    OpenAiChat,
    Harmony,
    GeminiRust,
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<dyn LlmClient>,
    provider: Provider,
    model: String,
}

impl Client {
    pub fn provider(&self) -> Provider {
        self.provider
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn set_model(&mut self, model: String) {
        self.model = model;
    }
}

#[async_trait]
impl LlmClient for Client {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        self.inner.send_chat_messages_stream(request).await
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        self.inner.list_models().await
    }
}

pub fn client_from(
    provider: Provider,
    model: String,
    host: Option<&str>,
) -> Result<Client, Box<dyn Error + Send + Sync>> {
    let inner: Arc<dyn LlmClient> = match provider {
        Provider::Ollama => Arc::new(ollama::OllamaClient::new(host)?),
        Provider::OpenAiChat => Arc::new(openai_chat::OpenAiChatClient::new(host)),
        Provider::Harmony => Arc::new(harmony::HarmonyClient::new(host)),
        Provider::GeminiRust => Arc::new(gemini_rust::GeminiRustClient::new(host)?),
    };
    Ok(Client {
        inner,
        provider,
        model,
    })
}

#[derive(Debug, Clone)]
pub enum ResponseChunk {
    Part(AssistantPart),
    Usage {
        input_tokens: u32,
        output_tokens: u32,
    },
    Done,
}

pub type ChatStream =
    Pin<Box<dyn Stream<Item = Result<ResponseChunk, Box<dyn Error + Send + Sync>>> + Send>>;

pub fn to_openapi_schema(schema: &Schema) -> Value {
    fn sanitize(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("$schema");
                if map.get("type") == Some(&Value::String("integer".into())) {
                    if let Some(Value::String(format)) = map.get_mut("format") {
                        let new_format = match format.as_str() {
                            "int32" | "int64" => format.clone(),
                            f if f.starts_with("uint") && f.contains("64") => "int64".to_string(),
                            f if f.starts_with("uint") && f.contains("32") => "int32".to_string(),
                            f if f.starts_with("uint") => "int64".to_string(),
                            f => f.to_string(),
                        };
                        *format = new_format;
                    }
                }
                for val in map.values_mut() {
                    sanitize(val);
                }
            }
            Value::Array(arr) => {
                for val in arr {
                    sanitize(val);
                }
            }
            _ => {}
        }
    }
    let mut value = to_value(schema).unwrap_or(Value::Null);
    sanitize(&mut value);
    value
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>>;

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>>;
}

#[cfg(test)]
mod tests {
    #![allow(dead_code)]
    use super::*;
    use schemars::{self, JsonSchema};

    #[derive(JsonSchema)]
    struct Params {
        value: u32,
    }

    #[test]
    fn unsigned_integers_use_signed_format() {
        let schema = schemars::schema_for!(Params);
        let value = to_openapi_schema(&schema);
        assert_eq!(
            value["properties"]["value"]["format"],
            Value::String("int32".to_string())
        );
        assert!(value.get("$schema").is_none());
    }
}
