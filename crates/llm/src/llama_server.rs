use std::error::Error;

use async_trait::async_trait;
use futures_util::StreamExt;
use openai_harmony::{
    HarmonyEncoding, HarmonyEncodingName, StreamableParser,
    chat::{Author, Message as HarmonyMessage, Role as HarmonyRole},
    load_harmony_encoding,
};
use reqwest::Client;
use reqwest_eventsource::{Event, RequestBuilderExt};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::{
    ChatMessage, ChatMessageRequest, ChatStream, LlmClient, ResponseChunk, ResponseMessage,
};

pub struct LlamaServerClient {
    host: String,
    http: Client,
    encoding: HarmonyEncoding,
}

impl LlamaServerClient {
    pub fn new(host: Option<&str>) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let host = host.unwrap_or("http://localhost:8000").to_string();
        let encoding = load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss)?;
        Ok(Self {
            host,
            http: Client::new(),
            encoding,
        })
    }
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct GenerateResponse {
    #[serde(default)]
    token: Option<u32>,
    #[serde(default)]
    done: bool,
}

#[async_trait]
impl LlmClient for LlamaServerClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let messages: Vec<HarmonyMessage> = request
            .messages
            .into_iter()
            .map(|m| match m {
                ChatMessage::User(u) => {
                    HarmonyMessage::from_role_and_content(HarmonyRole::User, u.content)
                }
                ChatMessage::Assistant(a) => {
                    HarmonyMessage::from_role_and_content(HarmonyRole::Assistant, a.content)
                }
                ChatMessage::System(s) => {
                    HarmonyMessage::from_role_and_content(HarmonyRole::System, s.content)
                }
                ChatMessage::Tool(t) => {
                    let content_str = match t.content {
                        serde_json::Value::String(s) => s,
                        v => v.to_string(),
                    };
                    HarmonyMessage::from_author_and_content(
                        Author::new(HarmonyRole::Tool, t.tool_name),
                        content_str,
                    )
                }
            })
            .collect();

        let rendered_tokens = self.encoding.render_conversation_for_completion(
            &messages,
            HarmonyRole::Assistant,
            None,
        )?;
        let rendered = self.encoding.tokenizer().decode_utf8(&rendered_tokens)?;

        let body = GenerateRequest {
            model: &request.model_name,
            prompt: &rendered,
            stream: true,
        };

        let url = format!("{}/v1/generate", self.host);
        let event_source = self.http.post(url).json(&body).eventsource()?;
        let encoding = self.encoding.clone();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            let mut parser = match StreamableParser::new(encoding, Some(HarmonyRole::Assistant)) {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(Err(e.into()));
                    return;
                }
            };
            let mut es = event_source;
            while let Some(ev) = es.next().await {
                match ev {
                    Err(e) => {
                        let _ = tx.send(Err(e.into()));
                        break;
                    }
                    Ok(Event::Open) => continue,
                    Ok(Event::Message(msg)) => {
                        if msg.data == "[DONE]" {
                            let _ = tx.send(Ok(ResponseChunk {
                                message: ResponseMessage {
                                    content: None,
                                    tool_calls: Vec::new(),
                                    thinking: None,
                                },
                                done: true,
                                usage: None,
                            }));
                            break;
                        }
                        match serde_json::from_str::<GenerateResponse>(&msg.data) {
                            Ok(data) => {
                                if let Some(t) = data.token {
                                    if let Err(e) = parser.process(t) {
                                        let _ = tx.send(Err(e.into()));
                                        break;
                                    }
                                    if let Ok(Some(delta)) = parser.last_content_delta() {
                                        let _ = tx.send(Ok(ResponseChunk {
                                            message: ResponseMessage {
                                                content: Some(delta),
                                                tool_calls: Vec::new(),
                                                thinking: None,
                                            },
                                            done: false,
                                            usage: None,
                                        }));
                                    }
                                }
                                if data.done {
                                    let _ = tx.send(Ok(ResponseChunk {
                                        message: ResponseMessage {
                                            content: None,
                                            tool_calls: Vec::new(),
                                            thinking: None,
                                        },
                                        done: true,
                                        usage: None,
                                    }));
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(e.into()));
                                break;
                            }
                        }
                    }
                }
            }
            es.close();
        });

        Ok(Box::pin(UnboundedReceiverStream::new(rx)))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Ok(vec!["gpt-oss".to_string()])
    }
}
