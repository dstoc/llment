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
    AssistantMessage, ChatMessage, ChatMessageRequest, LlmClient, ResponseChunk, ResponseMessage,
    ToolCall,
};

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn call(&self, name: &str, args: Value) -> Result<String, Box<dyn Error + Send + Sync>>;
}

pub enum ToolEvent {
    Chunk(ResponseChunk),
    ToolStarted {
        id: usize,
        name: String,
        args: Value,
    },
    ToolResult {
        id: usize,
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
    let mut next_id = 0usize;
    loop {
        let mut stream = client.send_chat_messages_stream(request.clone()).await?;
        let mut handles: JoinSet<(
            usize,
            String,
            String,
            Result<String, Box<dyn Error + Send + Sync>>,
        )> = JoinSet::new();
        let mut assistant_content: Option<String> = None;
        let mut assistant_thinking: Option<String> = None;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let mut done = false;
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            match &chunk {
                ResponseChunk::Message(ResponseMessage::Content(content)) => {
                    if !content.is_empty() {
                        assistant_content
                            .get_or_insert_with(String::new)
                            .push_str(content);
                    }
                }
                ResponseChunk::Message(ResponseMessage::Thinking(thinking)) => {
                    if !thinking.is_empty() {
                        assistant_thinking
                            .get_or_insert_with(String::new)
                            .push_str(thinking);
                    }
                }
                ResponseChunk::Message(ResponseMessage::ToolCall(tc)) => {
                    tool_calls.push(tc.clone());
                }
                ResponseChunk::Usage { .. } => {}
                ResponseChunk::Done => {
                    done = true;
                }
            }
            if !tool_calls.is_empty() {
                if let Some(content) = assistant_content.take() {
                    chat_history
                        .lock()
                        .unwrap()
                        .push(ChatMessage::Assistant(AssistantMessage {
                            content,
                            tool_calls: vec![],
                            thinking: None,
                        }));
                }
                chat_history
                    .lock()
                    .unwrap()
                    .push(ChatMessage::Assistant(AssistantMessage {
                        content: "".into(),
                        tool_calls: tool_calls.clone(),
                        thinking: assistant_thinking.take(),
                    }));
            }
            tx.send(ToolEvent::Chunk(chunk)).ok();
            for call in tool_calls {
                let event_id = next_id;
                next_id += 1;
                tx.send(ToolEvent::ToolStarted {
                    id: event_id,
                    name: call.name.clone(),
                    args: call.arguments.clone(),
                })
                .ok();
                let executor = tool_executor.clone();
                let name = call.name.clone();
                let args = call.arguments.clone();
                let call_id = call.id.clone();
                handles.spawn(async move {
                    let res = executor.call(&name, args).await;
                    (event_id, name, call_id, res)
                });
            }
            if done {
                break;
            }
        }
        if assistant_content.is_some() || assistant_thinking.is_some() {
            chat_history
                .lock()
                .unwrap()
                .push(ChatMessage::Assistant(AssistantMessage {
                    content: assistant_content.unwrap_or_default(),
                    tool_calls: Vec::new(),
                    thinking: assistant_thinking,
                }));
        }
        if handles.is_empty() {
            break;
        }
        while let Some(res) = handles.join_next().await {
            if let Ok((event_id, name, call_id, result)) = res {
                match &result {
                    Ok(text) => {
                        let content = serde_json::from_str::<Value>(&text)
                            .unwrap_or_else(|_| Value::String(text.clone()));
                        chat_history.lock().unwrap().push(ChatMessage::tool(
                            call_id.clone(),
                            content,
                            name.clone(),
                        ));
                    }
                    Err(err) => chat_history.lock().unwrap().push(ChatMessage::tool(
                        call_id.clone(),
                        Value::String(format!("Tool Failed: {}", err)),
                        name.clone(),
                    )),
                }
                tx.send(ToolEvent::ToolResult {
                    id: event_id,
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
                    Ok(ResponseChunk::Message(ResponseMessage::Content(
                        "first".into(),
                    ))),
                    Ok(ResponseChunk::Message(ResponseMessage::ToolCall(
                        crate::ToolCall {
                            id: "call-1".into(),
                            name: "test".into(),
                            arguments: Value::Null,
                        },
                    ))),
                    Ok(ResponseChunk::Done),
                ],
                2 => vec![
                    Ok(ResponseChunk::Message(ResponseMessage::Content(
                        "final".into(),
                    ))),
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
        // Behavior: assistant preamble content, then separate tool-call message, tool result, and final assistant response
        assert_eq!(updated.len(), 5);
        // First assistant message should contain the preamble content with no tool calls
        let preamble_msg = &updated[1];
        if let ChatMessage::Assistant(a) = preamble_msg {
            assert_eq!(a.tool_calls.len(), 0);
            assert_eq!(a.content, "first");
        } else {
            panic!("expected assistant preamble message");
        }
        // Next assistant message should carry the tool call with empty content
        let call_msg = &updated[2];
        if let ChatMessage::Assistant(a) = call_msg {
            assert_eq!(a.tool_calls.len(), 1);
            assert_eq!(a.tool_calls[0].name, "test");
            assert_eq!(a.content, "");
        } else {
            panic!("expected assistant message with tool call");
        }
        let final_msg = updated.last().unwrap();
        if let ChatMessage::Assistant(a) = final_msg {
            assert_eq!(a.content, "final");
        } else {
            panic!("expected assistant message");
        }
        // collect events
        let mut saw_final = false;
        let mut saw_tool = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                ToolEvent::ToolResult { .. } => saw_tool = true,
                ToolEvent::Chunk(ResponseChunk::Message(ResponseMessage::Content(content)))
                    if content == "final" =>
                {
                    saw_final = true
                }
                _ => {}
            }
        }
        assert!(saw_tool);
        assert!(saw_final);
    }
}
