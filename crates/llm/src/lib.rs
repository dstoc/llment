use std::error::Error;
use std::pin::Pin;

use async_trait::async_trait;
use serde_json::{Value, to_value};
use tokio_stream::Stream;

pub use ollama_rs::{
    generation::{
        chat::{ChatMessage, MessageRole, request::ChatMessageRequest},
        tools::{ToolCall, ToolCallFunction, ToolFunctionInfo, ToolInfo, ToolType},
    },
    re_exports::schemars::Schema,
};

pub mod gemini;
pub mod ollama;
pub mod openai;
pub mod tools;

#[derive(Debug)]
pub struct ResponseMessage {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub thinking: Option<String>,
}

#[derive(Debug)]
pub struct ResponseChunk {
    pub message: ResponseMessage,
    pub done: bool,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use ollama_rs::re_exports::schemars::{self as schemars, JsonSchema};

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
