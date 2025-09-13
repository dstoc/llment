use std::{
    error::Error,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use serde_json::Value;
use tokio::{
    sync::mpsc::UnboundedSender,
    task::{JoinHandle, JoinSet},
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::{Stream, StreamExt};

use crate::{
    AssistantMessage, AssistantPart, ChatMessage, ChatMessageRequest, JsonResult, LlmClient,
    ResponseChunk,
};

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn call(&self, name: &str, args: Value) -> Result<String, Box<dyn Error + Send + Sync>>;
}

pub enum ToolEvent {
    RequestStarted,
    Chunk(ResponseChunk),
    ToolStarted {
        call_id: String,
        name: String,
        args: JsonResult,
    },
    ToolResult {
        call_id: String,
        name: String,
        result: Result<String, Box<dyn Error + Send + Sync>>,
    },
}

pub fn tool_event_stream(
    client: Arc<dyn LlmClient>,
    request: ChatMessageRequest,
    tool_executor: Arc<dyn ToolExecutor>,
    chat_history: Arc<Mutex<Vec<ChatMessage>>>,
) -> (
    impl Stream<Item = ToolEvent>,
    JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(run_tool_loop(
        client,
        request,
        tool_executor,
        chat_history,
        tx,
    ));
    (UnboundedReceiverStream::new(rx), handle)
}

pub async fn run_tool_loop(
    client: Arc<dyn LlmClient>,
    mut request: ChatMessageRequest,
    tool_executor: Arc<dyn ToolExecutor>,
    chat_history: Arc<Mutex<Vec<ChatMessage>>>,
    tx: UnboundedSender<ToolEvent>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        let mut stream = client.send_chat_messages_stream(request.clone()).await?;
        tx.send(ToolEvent::RequestStarted).ok();
        let mut handles: JoinSet<(String, String, Result<String, Box<dyn Error + Send + Sync>>)> =
            JoinSet::new();
        let mut parts: Vec<AssistantPart> = Vec::new();
        let mut current_part: Option<AssistantPart> = None;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let mut done = false;
            match &chunk {
                ResponseChunk::Content(content) => {
                    if let Some(AssistantPart::Text { text }) = current_part.as_mut() {
                        text.push_str(content);
                    } else {
                        if let Some(part) = current_part.take() {
                            parts.push(part);
                        }
                        current_part = Some(AssistantPart::Text {
                            text: content.into(),
                        });
                    }
                }
                ResponseChunk::Thinking(thinking) => {
                    if let Some(AssistantPart::Thinking { text }) = current_part.as_mut() {
                        text.push_str(thinking);
                    } else {
                        if let Some(part) = current_part.take() {
                            parts.push(part);
                        }
                        current_part = Some(AssistantPart::Thinking {
                            text: thinking.into(),
                        });
                    }
                }
                ResponseChunk::ToolCall(tc) => {
                    if let Some(part) = current_part.take() {
                        parts.push(part);
                    }
                    current_part = Some(AssistantPart::ToolCall(tc.clone()));
                    tx.send(ToolEvent::ToolStarted {
                        call_id: tc.id.clone(),
                        name: tc.name.clone(),
                        args: tc.arguments.clone(),
                    })
                    .ok();
                    let executor = tool_executor.clone();
                    let name = tc.name.clone();
                    let args = tc.arguments.clone();
                    let call_id = tc.id.clone();
                    handles.spawn(async move {
                        match args {
                            JsonResult::Content { content } => {
                                let res = executor.call(&name, content).await;
                                (call_id, name, res)
                            }
                            JsonResult::Error { .. } => (
                                call_id,
                                name,
                                Err::<String, Box<dyn Error + Send + Sync>>(Box::new(
                                    std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        "Could not parse arguments as JSON",
                                    ),
                                )),
                            ),
                        }
                    });
                }
                ResponseChunk::Usage { .. } => {}
                ResponseChunk::Done => {
                    done = true;
                }
            }
            tx.send(ToolEvent::Chunk(chunk)).ok();
            if done {
                break;
            }
        }
        if let Some(part) = current_part.take() {
            parts.push(part);
        }
        if !parts.is_empty() {
            chat_history
                .lock()
                .unwrap()
                .push(ChatMessage::Assistant(AssistantMessage { content: parts }));
        }
        if handles.is_empty() {
            break;
        }
        while let Some(res) = handles.join_next().await {
            if let Ok((call_id, name, result)) = res {
                match &result {
                    Ok(text) => {
                        let content = serde_json::from_str::<Value>(&text)
                            .unwrap_or_else(|_| Value::String(text.clone()));
                        chat_history.lock().unwrap().push(ChatMessage::tool(
                            call_id.clone(),
                            JsonResult::Content { content },
                            name.clone(),
                        ));
                    }
                    Err(err) => chat_history.lock().unwrap().push(ChatMessage::tool(
                        call_id.clone(),
                        JsonResult::Error {
                            error: format!("Tool Failed: {}", err),
                        },
                        name.clone(),
                    )),
                }
                tx.send(ToolEvent::ToolResult {
                    call_id,
                    name,
                    result,
                })
                .ok();
            }
        }
        let history_clone = { chat_history.lock().unwrap().clone() };
        request = ChatMessageRequest::new(request.model_name.clone(), history_clone)
            .tools(request.tools.clone())
            .think(true);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JsonResult;
    use serde_json::Value;
    use std::sync::{Arc, Mutex};
    use tokio_stream::{self};

    struct DummyClient {
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl LlmClient for DummyClient {
        async fn send_chat_messages_stream(
            &self,
            _request: ChatMessageRequest,
        ) -> Result<crate::ChatStream, Box<dyn Error + Send + Sync>> {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            let stream: Vec<Result<ResponseChunk, Box<dyn Error + Send + Sync>>> = match *calls {
                1 => vec![
                    Ok(ResponseChunk::Content("first".into())),
                    Ok(ResponseChunk::ToolCall(crate::ToolCall {
                        id: "call-1".into(),
                        name: "test".into(),
                        arguments: JsonResult::Content {
                            content: Value::Null,
                        },
                    })),
                    Ok(ResponseChunk::Done),
                ],
                2 => vec![
                    Ok(ResponseChunk::Content("final".into())),
                    Ok(ResponseChunk::Done),
                ],
                _ => vec![],
            };
            Ok(Box::pin(tokio_stream::iter(stream)))
        }

        async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
            Ok(vec![])
        }
    }

    struct DummyExecutor;

    #[async_trait]
    impl ToolExecutor for DummyExecutor {
        async fn call(
            &self,
            name: &str,
            _args: Value,
        ) -> Result<String, Box<dyn Error + Send + Sync>> {
            Ok(format!("called {name}"))
        }
    }

    #[tokio::test]
    async fn executes_tool_and_follow_up() {
        let client = Arc::new(DummyClient {
            calls: Mutex::new(0),
        });
        let exec = Arc::new(DummyExecutor);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let history = Arc::new(Mutex::new(vec![ChatMessage::user("hi".to_string())]));
        let request_history = { history.lock().unwrap().clone() };
        let request = ChatMessageRequest::new("m".into(), request_history).think(true);
        run_tool_loop(client, request, exec, history.clone(), tx)
            .await
            .unwrap();
        let updated = history.lock().unwrap().clone();
        // Behavior: assistant content and tool call are combined in one assistant message,
        // followed by the tool result message and the final assistant response
        assert_eq!(updated.len(), 4);
        // First assistant message should contain the preamble content followed by the tool call
        let first_assistant = &updated[1];
        if let ChatMessage::Assistant(a) = first_assistant {
            assert_eq!(a.content.len(), 2);
            match &a.content[0] {
                AssistantPart::Text { text } => assert_eq!(text, "first"),
                _ => panic!("expected first part to be text"),
            }
            match &a.content[1] {
                AssistantPart::ToolCall(tc) => assert_eq!(tc.name, "test"),
                _ => panic!("expected second part to be tool call"),
            }
        } else {
            panic!("expected combined assistant message");
        }
        // Final assistant message should be the follow-up content
        let final_msg = updated.last().unwrap();
        if let ChatMessage::Assistant(a) = final_msg {
            assert_eq!(a.content.len(), 1);
            match &a.content[0] {
                AssistantPart::Text { text } => assert_eq!(text, "final"),
                _ => panic!("expected text part"),
            }
        } else {
            panic!("expected assistant message");
        }
        // collect events
        let mut saw_final = false;
        let mut saw_tool = false;
        let mut requests = 0;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                ToolEvent::ToolResult { .. } => saw_tool = true,
                ToolEvent::Chunk(ResponseChunk::Content(content)) if content == "final" => {
                    saw_final = true
                }
                ToolEvent::RequestStarted => requests += 1,
                _ => {}
            }
        }
        assert!(saw_tool);
        assert!(saw_final);
        assert_eq!(requests, 2);
    }

    struct InvalidClient {
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl LlmClient for InvalidClient {
        async fn send_chat_messages_stream(
            &self,
            _request: ChatMessageRequest,
        ) -> Result<crate::ChatStream, Box<dyn Error + Send + Sync>> {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            let stream: Vec<Result<ResponseChunk, Box<dyn Error + Send + Sync>>> = match *calls {
                1 => vec![
                    Ok(ResponseChunk::ToolCall(crate::ToolCall {
                        id: "call-1".into(),
                        name: "test".into(),
                        arguments: JsonResult::Error {
                            error: "nope".into(),
                        },
                    })),
                    Ok(ResponseChunk::Done),
                ],
                2 => vec![
                    Ok(ResponseChunk::Content("final".into())),
                    Ok(ResponseChunk::Done),
                ],
                _ => vec![],
            };
            Ok(Box::pin(tokio_stream::iter(stream)))
        }

        async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
            Ok(vec![])
        }
    }

    struct CountingExecutor {
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl ToolExecutor for CountingExecutor {
        async fn call(
            &self,
            _name: &str,
            _args: Value,
        ) -> Result<String, Box<dyn Error + Send + Sync>> {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            Ok("should not be called".into())
        }
    }

    #[tokio::test]
    async fn skips_executor_on_invalid_args() {
        let client = Arc::new(InvalidClient {
            calls: Mutex::new(0),
        });
        let exec = Arc::new(CountingExecutor {
            calls: Mutex::new(0),
        });
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let history = Arc::new(Mutex::new(vec![ChatMessage::user("hi".to_string())]));
        let request_history = { history.lock().unwrap().clone() };
        let request = ChatMessageRequest::new("m".into(), request_history).think(true);
        run_tool_loop(client, request, exec.clone(), history.clone(), tx)
            .await
            .unwrap();
        assert_eq!(*exec.calls.lock().unwrap(), 0);
        let updated = history.lock().unwrap().clone();
        assert_eq!(updated.len(), 4);
        if let ChatMessage::Tool(t) = &updated[2] {
            match &t.content {
                JsonResult::Error { error } => {
                    assert_eq!(error, "Tool Failed: Could not parse arguments as JSON")
                }
                _ => panic!("expected tool failure message"),
            }
        } else {
            panic!("expected tool failure message");
        }
        let mut saw_error = false;
        while let Ok(ev) = rx.try_recv() {
            if let ToolEvent::ToolResult { result, .. } = ev {
                if let Err(err) = result {
                    if err.to_string() == "Could not parse arguments as JSON" {
                        saw_error = true;
                    }
                }
            }
        }
        assert!(saw_error);
    }
}
