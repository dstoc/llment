use std::error::Error;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use clap::ValueEnum;
use schemars::Schema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, to_value};
use tokio_stream::Stream;

fn option_string_is_empty(value: &Option<String>) -> bool {
    match value {
        None => true,
        Some(s) => s.is_empty(),
    }
}

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
            content,
            tool_calls: Vec::new(),
            thinking: None,
        })
    }

    pub fn system(content: String) -> Self {
        Self::System(SystemMessage { content })
    }

    pub fn tool(id: String, content: Value, tool_name: String) -> Self {
        Self::Tool(ToolMessage {
            id,
            content,
            tool_name,
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(skip_serializing_if = "option_string_is_empty", default)]
    pub thinking: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemMessage {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolMessage {
    pub id: String,
    pub content: Value,
    pub tool_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(with = "value_or_string")]
    pub arguments: Result<Value, String>,
}

mod value_or_string {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(value: &Result<Value, String>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Ok(v) => v.serialize(serializer),
            Err(s) => s.serialize(serializer),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Result<Value, String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        match v {
            Value::String(s) => Ok(Err(s)),
            other => Ok(Ok(other)),
        }
    }
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

pub mod gemini;
pub mod gpt_oss;
pub mod mcp;
pub mod ollama;
pub mod openai;
pub mod test_provider;
pub mod tools;

pub use test_provider::TestProvider;

#[derive(Default, Copy, Clone, Debug, ValueEnum)]
pub enum Provider {
    #[default]
    Ollama,
    Openai,
    GptOss,
    Gemini,
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
        Provider::Openai => Arc::new(openai::OpenAiClient::new(host)),
        Provider::GptOss => Arc::new(gpt_oss::GptOssClient::new(host)),
        Provider::Gemini => Arc::new(gemini::GeminiClient::new(host)),
    };
    Ok(Client {
        inner,
        provider,
        model,
    })
}

#[derive(Debug, Clone)]
pub enum ResponseChunk {
    Thinking(String),
    ToolCall(ToolCall),
    Content(String),
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
    use super::*;
    use schemars::{self, JsonSchema};
    use serde_json::json;

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

    #[test]
    fn tool_call_arguments_roundtrip_ok() {
        let call = ToolCall {
            id: "1".into(),
            name: "test".into(),
            arguments: Ok(json!({ "a": 1 })),
        };
        let serialized = serde_json::to_string(&call).unwrap();
        let parsed: ToolCall = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed.arguments, Ok(json!({ "a": 1 })));
    }

    #[test]
    fn tool_call_arguments_roundtrip_err() {
        let call = ToolCall {
            id: "1".into(),
            name: "test".into(),
            arguments: Err("not-json".into()),
        };
        let serialized = serde_json::to_string(&call).unwrap();
        let parsed: ToolCall = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed.arguments, Err("not-json".into()));
    }
}
