use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use tokio::{
    sync::mpsc::UnboundedSender,
    task::{JoinHandle, JoinSet},
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::{Stream, StreamExt};

use crate::{ChatMessage, ChatMessageRequest, LlmClient, ResponseChunk};

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
    chat_history: Vec<ChatMessage>,
) -> (
    impl Stream<Item = ToolEvent>,
    JoinHandle<Result<Vec<ChatMessage>, Box<dyn Error + Send + Sync>>>,
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
    mut chat_history: Vec<ChatMessage>,
    tx: UnboundedSender<ToolEvent>,
) -> Result<Vec<ChatMessage>, Box<dyn Error + Send + Sync>> {
    let mut next_id = 0usize;
    loop {
        let mut stream = client.send_chat_messages_stream(request.clone()).await?;
        let mut handles: JoinSet<(usize, String, Result<String, Box<dyn Error + Send + Sync>>)> =
            JoinSet::new();
        let mut assistant_content: Option<String> = None;
        let mut assistant_thinking: Option<String> = None;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if let Some(ref c) = chunk.message.content {
                if !c.is_empty() {
                    assistant_content
                        .get_or_insert_with(String::new)
                        .push_str(c);
                }
            }
            if let Some(ref c) = chunk.message.thinking {
                // TODO: sometimes there's assistant_content at this time, seems like
                // a model bug, we could merge it into thinking at this point?
                if !c.is_empty() {
                    assistant_thinking
                        .get_or_insert_with(String::new)
                        .push_str(c);
                }
            }
            let done = chunk.done;
            let tool_calls = chunk.message.tool_calls.clone();
            if !tool_calls.is_empty() {
                let mut msg = ChatMessage::assistant(String::new());
                msg.tool_calls = tool_calls.clone();
                msg.thinking = assistant_thinking;
                chat_history.push(msg);
                assistant_thinking = None;
            }
            tx.send(ToolEvent::Chunk(chunk)).ok();
            for call in tool_calls {
                let id = next_id;
                next_id += 1;
                tx.send(ToolEvent::ToolStarted {
                    id,
                    name: call.function.name.clone(),
                    args: call.function.arguments.clone(),
                })
                .ok();
                let executor = tool_executor.clone();
                let name = call.function.name.clone();
                let args = call.function.arguments.clone();
                handles.spawn(async move {
                    let res = executor.call(&name, args).await;
                    (id, name, res)
                });
            }
            if done {
                break;
            }
        }
        if assistant_content.is_some() || assistant_thinking.is_some() {
            let mut msg = ChatMessage::assistant(assistant_content.unwrap_or_default());
            msg.thinking = assistant_thinking;
            chat_history.push(msg);
        }
        if handles.is_empty() {
            break;
        }
        while let Some(res) = handles.join_next().await {
            if let Ok((id, name, result)) = res {
                match &result {
                    Ok(text) => chat_history.push(ChatMessage::tool(text.clone(), name.clone())),
                    Err(err) => chat_history.push(ChatMessage::tool(
                        format!("Tool Failed: {}", err),
                        name.clone(),
                    )),
                }
                tx.send(ToolEvent::ToolResult { id, name, result }).ok();
            }
        }
        request = ChatMessageRequest::new(request.model_name.clone(), chat_history.clone())
            .tools(request.tools.clone())
            .think(true);
    }
    Ok(chat_history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageRole;
    use serde_json::Value;
    use std::sync::Mutex;
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
                1 => vec![Ok(ResponseChunk {
                    message: crate::ResponseMessage {
                        content: None,
                        thinking: None,
                        tool_calls: vec![crate::ToolCall {
                            function: crate::ToolCallFunction {
                                name: "test".into(),
                                arguments: Value::Null,
                            },
                        }],
                    },
                    done: true,
                    usage: None,
                })],
                2 => vec![Ok(ResponseChunk {
                    message: crate::ResponseMessage {
                        content: Some("final".into()),
                        thinking: None,
                        tool_calls: vec![],
                    },
                    done: true,
                    usage: None,
                })],
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
        let history = vec![ChatMessage::user("hi".to_string())];
        let request = ChatMessageRequest::new("m".into(), history.clone()).think(true);
        let updated = run_tool_loop(client, request, exec, history, tx)
            .await
            .unwrap();
        // ensure assistant tool call, tool result, and final assistant response added to history
        assert_eq!(updated.len(), 4);
        let call_msg = &updated[1];
        assert_eq!(call_msg.role, MessageRole::Assistant);
        assert_eq!(call_msg.tool_calls.len(), 1);
        assert_eq!(call_msg.tool_calls[0].function.name, "test");
        assert!(call_msg.content.is_empty());
        let final_msg = updated.last().unwrap();
        assert_eq!(final_msg.role, MessageRole::Assistant);
        assert_eq!(final_msg.content, "final");
        // collect events
        let mut saw_final = false;
        let mut saw_tool = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                ToolEvent::ToolResult { .. } => saw_tool = true,
                ToolEvent::Chunk(c) if c.message.content.as_deref() == Some("final") => {
                    saw_final = true
                }
                _ => {}
            }
        }
        assert!(saw_tool);
        assert!(saw_final);
    }
}
