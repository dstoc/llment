use std::{error::Error, pin::Pin};

use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};
use tokio_stream::{Stream, StreamExt};

/// Request payload for the llama-server `/completion` endpoint.
#[derive(Serialize)]
pub struct CompletionRequest {
    pub prompt: Vec<u32>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammar: Option<String>,
}

/// Streamed response chunk from the llama-server `/completion` endpoint.
#[derive(Default, Deserialize)]
pub struct CompletionResponse {
    #[serde(default)]
    pub tokens: Vec<u32>,
    #[serde(default)]
    pub stop: bool,
}

pub type CompletionStream =
    Pin<Box<dyn Stream<Item = Result<CompletionResponse, Box<dyn Error + Send + Sync>>> + Send>>;

/// Create a [`CompletionStream`] for the llama-server `/completion` endpoint.
pub async fn llama_server_completion(
    client: &reqwest::Client,
    host: &str,
    request: CompletionRequest,
) -> Result<CompletionStream, Box<dyn Error + Send + Sync>> {
    let url = format!("{}/completion", host.trim_end_matches('/'));
    let req = client.post(url).json(&request);
    let es = EventSource::new(req)?;
    let stream = es.filter_map(|event| match event {
        Ok(Event::Message(msg)) => {
            if msg.data == "[DONE]" {
                None
            } else {
                Some(
                    serde_json::from_str::<CompletionResponse>(&msg.data)
                        .map_err(|e| Box::<dyn Error + Send + Sync>::from(e)),
                )
            }
        }
        Ok(Event::Open) => None,
        Err(e) => Some(Err(Box::<dyn Error + Send + Sync>::from(e))),
    });
    Ok(Box::pin(stream))
}
