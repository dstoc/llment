use std::collections::VecDeque;
use std::error::Error;
use std::sync::Mutex;

use async_trait::async_trait;
use tokio_stream::iter;

use crate::{ChatMessageRequest, ChatStream, LlmClient, ResponseChunk};

pub struct TestProvider {
    pub requests: Mutex<Vec<ChatMessageRequest>>,
    responses: Mutex<VecDeque<Vec<ResponseChunk>>>,
}

impl TestProvider {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::new()),
        }
    }

    pub fn enqueue(&self, chunks: Vec<ResponseChunk>) {
        self.responses.lock().unwrap().push_back(chunks);
    }
}

#[async_trait]
impl LlmClient for TestProvider {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        self.requests.lock().unwrap().push(request);
        let chunks = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default()
            .into_iter()
            .map(Ok);
        Ok(Box::pin(iter(chunks)))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolExecutor, run_tool_loop};
    use crate::{ChatMessage, MessageRole, ResponseMessage, ToolCall};
    use serde_json::Value;
    use std::sync::Arc;

    struct DummyExec;

    #[async_trait]
    impl ToolExecutor for DummyExec {
        async fn call(
            &self,
            name: &str,
            _args: Value,
        ) -> Result<String, Box<dyn Error + Send + Sync>> {
            Ok(format!("called {name}"))
        }
    }

    #[tokio::test]
    async fn captures_requests_and_iterates() {
        let client = Arc::new(TestProvider::new());
        client.enqueue(vec![ResponseChunk {
            message: ResponseMessage {
                content: None,
                tool_calls: vec![ToolCall {
                    name: "test".into(),
                    arguments: Value::Null,
                }],
                thinking: None,
            },
            done: true,
            usage: None,
        }]);
        client.enqueue(vec![ResponseChunk {
            message: ResponseMessage {
                content: Some("final".into()),
                tool_calls: vec![],
                thinking: None,
            },
            done: true,
            usage: None,
        }]);
        let exec = Arc::new(DummyExec);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let history = vec![ChatMessage::user("hi".into())];
        let request = ChatMessageRequest::new("m".into(), history.clone()).think(true);
        let updated = run_tool_loop(client.clone(), request, exec, history, tx)
            .await
            .unwrap();
        let requests = client.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].messages.len(), 1);
        assert_eq!(requests[1].messages.len(), 3);
        let final_msg = updated.last().unwrap();
        assert_eq!(final_msg.role, MessageRole::Assistant);
        assert_eq!(final_msg.content, "final");
    }
}
