use std::error::Error;

use async_trait::async_trait;
use openai_harmony::{
    HarmonyEncodingName,
    chat::{Conversation, Message as HarmonyMessage, Role},
    load_harmony_encoding,
};
use reqwest::Client as HttpClient;
use tokio_stream::once;

use crate::{
    ChatMessage, ChatMessageRequest, ChatStream, LlmClient, ResponseChunk, ResponseMessage,
};

pub struct LlamaServerClient {
    host: String,
    http: HttpClient,
}

impl LlamaServerClient {
    pub fn new(host: Option<&str>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Ok(Self {
            host: host.unwrap_or("http://localhost:8000").to_string(),
            http: HttpClient::new(),
        })
    }
}

#[derive(serde::Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[async_trait]
impl LlmClient for LlamaServerClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let enc = load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss)?;
        let mut msgs = Vec::new();
        for msg in request.messages {
            match msg {
                ChatMessage::User(u) => {
                    msgs.push(HarmonyMessage::from_role_and_content(Role::User, u.content));
                }
                ChatMessage::Assistant(a) => {
                    msgs.push(HarmonyMessage::from_role_and_content(
                        Role::Assistant,
                        a.content,
                    ));
                }
                ChatMessage::System(s) => {
                    msgs.push(HarmonyMessage::from_role_and_content(
                        Role::System,
                        s.content,
                    ));
                }
                ChatMessage::Tool(t) => {
                    msgs.push(HarmonyMessage::from_role_and_content(
                        Role::Tool,
                        t.content.to_string(),
                    ));
                }
            }
        }
        let convo = Conversation::from_messages(msgs);
        let tokens = enc.render_conversation_for_completion(&convo, Role::Assistant, None)?;
        let prompt = enc.tokenizer().decode_utf8(&tokens)?;

        let body = GenerateRequest {
            model: request.model_name,
            prompt,
            stream: false,
        };
        let url = format!("{}/v1/generate", self.host);
        let resp: serde_json::Value = self.http.post(url).json(&body).send().await?.json().await?;
        let text = resp["content"]
            .as_str()
            .or_else(|| resp["response"].as_str())
            .or_else(|| resp["text"].as_str())
            .or_else(|| resp["choices"].get(0).and_then(|c| c["text"].as_str()))
            .unwrap_or_default()
            .to_string();
        let resp_tokens = enc.tokenizer().encode_ordinary(&text);
        let _ = enc.parse_messages_from_completion_tokens(resp_tokens, Some(Role::Assistant))?;

        let chunk = ResponseChunk {
            message: ResponseMessage {
                content: Some(text),
                tool_calls: Vec::new(),
                thinking: None,
            },
            done: true,
            usage: None,
        };
        Ok(Box::pin(once(Ok(chunk))))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Ok(vec!["gpt-oss".to_string()])
    }
}
