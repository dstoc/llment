use std::error::Error;

use reqwest_eventsource::EventSource;

/// Create an [`EventSource`] for the llama-server `/completions` endpoint.
pub async fn llama_server_completions(
    client: &reqwest::Client,
    host: &str,
    request: async_openai::types::CreateCompletionRequest,
) -> Result<EventSource, Box<dyn Error + Send + Sync>> {
    let url = format!("{}/completions", host.trim_end_matches('/'));
    let req = client.post(url).json(&request);
    Ok(EventSource::new(req)?)
}
