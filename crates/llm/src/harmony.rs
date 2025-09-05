use std::error::Error;

use super::{
    ChatMessage, ChatMessageRequest, ChatStream, LlmClient, ResponseChunk, ToolCall,
    to_openapi_schema,
};
use crate::llama_server::{CompletionRequest, llama_server_completion};
use async_trait::async_trait;
use futures_util::StreamExt;
use openai_harmony::{
    HarmonyEncoding, HarmonyEncodingName, StreamableParser,
    chat::{
        Author, Content, Conversation, DeveloperContent, Message, Role, SystemContent, TextContent,
        ToolDescription,
    },
    load_harmony_encoding,
};
use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

pub struct HarmonyClient {
    http: Client,
    host: String,
}

impl HarmonyClient {
    pub fn new(host: Option<&str>) -> Self {
        let host = host
            .map(|h| h.trim_end_matches("/v1").trim_end_matches('/'))
            .unwrap_or("http://localhost:8000")
            .to_string();
        Self {
            http: Client::new(),
            host,
        }
    }
}

fn conversation_to_prompt(
    encoding: &HarmonyEncoding,
    conversation: &Conversation,
    prefill: Option<String>,
) -> Result<(Vec<u32>, Option<Vec<u32>>), Box<dyn Error + Send + Sync>> {
    let ends_with_assistant = matches!(
        conversation.messages.last().map(|m| m.author.role.clone()),
        Some(Role::Assistant)
    );
    let mut tokens = if ends_with_assistant && prefill.is_none() {
        encoding.render_conversation(conversation, None)?
    } else {
        encoding.render_conversation_for_completion(conversation, Role::Assistant, None)?
    };
    let mut prefill_tokens = None;
    if let Some(prefill_text) = prefill {
        let pf_tokens = encoding
            .tokenizer()
            .encode_with_special_tokens(&prefill_text);
        tokens.extend_from_slice(&pf_tokens);
        prefill_tokens = Some(pf_tokens);
    }
    Ok((tokens, prefill_tokens))
}

fn build_prompt(
    encoding: &HarmonyEncoding,
    request: &ChatMessageRequest,
) -> Result<(Vec<u32>, Option<Vec<u32>>), Box<dyn Error + Send + Sync>> {
    let mut system_msgs = Vec::new();
    let mut other_msgs = Vec::new();
    let mut developer = DeveloperContent::new();
    for msg in &request.messages {
        match msg {
            ChatMessage::System(s) => {
                if !s.content.is_empty() {
                    developer = developer.with_instructions(s.content.clone());
                }
            }
            other => other_msgs.push(other.clone()),
        }
    }
    if !request.tools.is_empty() {
        let tools: Vec<ToolDescription> = request
            .tools
            .iter()
            .map(|t| {
                ToolDescription::new(
                    t.name.clone(),
                    t.description.clone(),
                    Some(to_openapi_schema(&t.parameters)),
                )
            })
            .collect();
        developer = developer.with_function_tools(tools);
    }
    system_msgs.push(Message::from_role_and_content(
        Role::System,
        SystemContent::new(),
    ));
    if developer.instructions.is_some() || developer.tools.is_some() {
        system_msgs.push(Message::from_role_and_content(Role::Developer, developer));
    }
    let mut convo_msgs = system_msgs;
    for msg in &other_msgs {
        match msg {
            ChatMessage::User(u) => {
                convo_msgs.push(Message::from_role_and_content(
                    Role::User,
                    u.content.clone(),
                ));
            }
            ChatMessage::Assistant(a) => {
                if let Some(thinking) = &a.thinking {
                    if !thinking.is_empty() && a.content.is_empty() {
                        convo_msgs.push(
                            Message::from_role_and_content(Role::Assistant, thinking.clone())
                                .with_channel("analysis"),
                        );
                    }
                }
                for tc in &a.tool_calls {
                    let args = tc
                        .arguments_invalid
                        .clone()
                        .unwrap_or_else(|| tc.arguments.to_string());
                    convo_msgs.push(
                        Message::from_role_and_content(Role::Assistant, args)
                            .with_channel("commentary")
                            .with_recipient(format!("functions.{}", tc.name))
                            .with_content_type("<|constrain|>json"),
                    );
                }
                if !a.content.is_empty() {
                    convo_msgs.push(
                        Message::from_role_and_content(Role::Assistant, a.content.clone())
                            .with_channel("final"),
                    );
                }
            }
            ChatMessage::Tool(t) => {
                let content_str = match &t.content {
                    Value::String(s) => s.clone(),
                    v => v.to_string(),
                };
                convo_msgs.push(
                    Message::from_author_and_content(
                        Author::new(Role::Tool, format!("functions.{}", t.tool_name)),
                        content_str,
                    )
                    .with_channel("commentary")
                    .with_recipient("assistant"),
                );
            }
            ChatMessage::System(_) => {}
        }
    }
    let mut prefill: Option<String> = None;
    if let Some(ChatMessage::Assistant(a)) = other_msgs.last() {
        if a.tool_calls.is_empty() {
            let thinking = a.thinking.as_deref().unwrap_or("");
            let content = a.content.as_str();
            match (thinking.is_empty(), content.is_empty()) {
                (false, true) => {
                    convo_msgs.pop();
                    prefill = Some(format!("<|channel|>analysis<|message|>{}", thinking));
                }
                (true, false) => {
                    convo_msgs.pop();
                    prefill = Some(format!("<|channel|>final<|message|>{}", content));
                }
                (false, false) => {
                    convo_msgs.pop();
                    prefill = Some(format!(
                        "<|channel|>analysis<|message|>{}<|end|><|start|>assistant<|channel|>final<|message|>{}",
                        thinking, content
                    ));
                }
                _ => {}
            }
        }
    }
    let conversation = Conversation::from_messages(convo_msgs);
    conversation_to_prompt(encoding, &conversation, prefill)
}

#[async_trait]
impl LlmClient for HarmonyClient {
    async fn send_chat_messages_stream(
        &self,
        request: ChatMessageRequest,
    ) -> Result<ChatStream, Box<dyn Error + Send + Sync>> {
        let encoding = tokio::task::spawn_blocking(|| {
            load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss)
        })
        .await
        .map_err(|e| Box::<dyn Error + Send + Sync>::from(e))?
        .map_err(|e| Box::<dyn Error + Send + Sync>::from(e))?;
        let (prompt_tokens, prefill_tokens) = build_prompt(&encoding, &request)?;
        let input_tokens = prompt_tokens.len() as u32;
        let req = CompletionRequest {
            prompt: prompt_tokens,
            stream: true,
        };
        let event_stream = llama_server_completion(&self.http, &self.host, req).await?;
        let mut parser = StreamableParser::new(encoding.clone(), Some(Role::Assistant))?;
        if let Some(tokens) = &prefill_tokens {
            for t in tokens {
                parser.process(*t).ok();
            }
        }
        let mut seen = parser.messages().len();
        let mut output_tokens: u32 = 0;
        let mapped = event_stream.flat_map(move |res| match res {
            Ok(chunk) => {
                output_tokens += chunk.tokens.len() as u32;
                let mut out = vec![];
                if !chunk.content.is_empty() {
                    let tokens = encoding
                        .tokenizer()
                        .encode_with_special_tokens(&chunk.content);
                    for t in tokens {
                        parser.process(t).ok();
                        if let Some(delta) = parser.last_content_delta().ok().flatten() {
                            if !delta.is_empty() && parser.current_recipient().is_none() {
                                match parser.current_channel().as_deref() {
                                    Some("analysis") => {
                                        out.push(Ok(ResponseChunk::Thinking(delta)))
                                    }
                                    Some("final") => out.push(Ok(ResponseChunk::Content(delta))),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if chunk.stop {
                    parser.process_eos().ok();
                }
                let messages = parser.messages();
                while seen < messages.len() {
                    let msg = &messages[seen];
                    seen += 1;
                    if let Some(recipient) = &msg.recipient {
                        if let Some(name) = recipient.strip_prefix("functions.") {
                            if let Some(Content::Text(TextContent { text })) = msg.content.first() {
                                let (args, args_invalid) = match serde_json::from_str(text) {
                                    Ok(v) => (v, None),
                                    Err(_) => (Value::Null, Some(text.clone())),
                                };
                                out.push(Ok(ResponseChunk::ToolCall(ToolCall {
                                    id: Uuid::new_v4().to_string(),
                                    name: name.to_string(),
                                    arguments: args,
                                    arguments_invalid: args_invalid,
                                })));
                            }
                        }
                    }
                }
                if chunk.stop {
                    out.push(Ok(ResponseChunk::Usage {
                        input_tokens,
                        output_tokens,
                    }));
                    out.push(Ok(ResponseChunk::Done));
                }
                tokio_stream::iter(out)
            }
            Err(e) => tokio_stream::iter(vec![Err::<ResponseChunk, _>(e.into())]),
        });
        Ok(Box::pin(mapped))
    }

    async fn list_models(&self) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Ok(vec!["gpt-oss".to_string()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssistantMessage, ToolCall};
    use serde_json::json;

    fn prompt_and_prefill(
        encoding: &HarmonyEncoding,
        request: &ChatMessageRequest,
    ) -> (String, Option<Vec<u32>>) {
        let (prompt_tokens, prefill_tokens) = build_prompt(encoding, request).unwrap();
        let prompt = encoding
            .tokenizer()
            .decode_utf8(&prompt_tokens)
            .unwrap()
            .to_string();
        (prompt, prefill_tokens)
    }

    fn setup(messages: Vec<ChatMessage>) -> (HarmonyEncoding, String, Option<Vec<u32>>) {
        let encoding = load_harmony_encoding(HarmonyEncodingName::HarmonyGptOss).unwrap();
        let request = ChatMessageRequest::new("gpt-oss".into(), messages);
        let (prompt, prefill_tokens) = prompt_and_prefill(&encoding, &request);
        (encoding, prompt, prefill_tokens)
    }

    #[test]
    fn prefill_with_thinking() {
        let (_, prompt, prefill_tokens) = setup(vec![
            ChatMessage::user("Hi".into()),
            ChatMessage::Assistant(AssistantMessage {
                content: String::new(),
                tool_calls: vec![],
                thinking: Some("ponder".into()),
            }),
        ]);
        assert!(prefill_tokens.is_some());
        assert!(prompt.contains("<|start|>system<|message|>"));
        assert!(prompt.ends_with(
            "<|start|>user<|message|>Hi<|end|><|start|>assistant<|channel|>analysis<|message|>ponder"
        ));
    }

    #[test]
    fn prefill_with_content() {
        let (_, prompt, prefill_tokens) = setup(vec![
            ChatMessage::user("Hi".into()),
            ChatMessage::assistant("Hello".into()),
        ]);
        assert!(prefill_tokens.is_some());
        assert!(prompt.contains("<|start|>system<|message|>"));
        assert!(prompt.ends_with(
            "<|start|>user<|message|>Hi<|end|><|start|>assistant<|channel|>final<|message|>Hello"
        ));
    }

    #[test]
    fn thinking_and_content_history() {
        let (_, prompt, prefill_tokens) = setup(vec![
            ChatMessage::user("Hi".into()),
            ChatMessage::Assistant(AssistantMessage {
                content: "Hello".into(),
                tool_calls: vec![],
                thinking: Some("ponder".into()),
            }),
            ChatMessage::user("How are you?".into()),
            ChatMessage::Assistant(AssistantMessage {
                content: "I'm good".into(),
                tool_calls: vec![],
                thinking: Some("think".into()),
            }),
        ]);
        assert!(prefill_tokens.is_some());
        assert!(prompt.contains("<|start|>system<|message|>"));
        assert!(!prompt.contains("<|channel|>analysis<|message|>ponder"));
        assert!(prompt.ends_with(concat!(
            "<|start|>user<|message|>Hi<|end|>",
            "<|start|>assistant<|channel|>final<|message|>Hello<|end|>",
            "<|start|>user<|message|>How are you?<|end|>",
            "<|start|>assistant<|channel|>analysis<|message|>think<|end|>",
            "<|start|>assistant<|channel|>final<|message|>I'm good"
        )));
    }

    #[test]
    fn tool_call_and_response() {
        let (_, prompt, prefill_tokens) = setup(vec![
            ChatMessage::user("2+2?".into()),
            ChatMessage::Assistant(AssistantMessage {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: "1".into(),
                    name: "add".into(),
                    arguments: json!({"a": 2, "b": 2}),
                    arguments_invalid: None,
                }],
                thinking: None,
            }),
            ChatMessage::tool("1".into(), json!({"sum": 4}), "add".into()),
        ]);
        assert!(prefill_tokens.is_none());
        let args = json!({"a": 2, "b": 2}).to_string();
        let result = json!({"sum": 4}).to_string();
        let expected_tail = format!(
            concat!(
                "<|start|>assistant to=functions.add<|channel|>commentary <|constrain|>json<|message|>{args}<|call|>",
                "<|start|>functions.add to=assistant<|channel|>commentary<|message|>{result}<|end|><|start|>assistant"
            ),
            args = args,
            result = result
        );
        assert!(prompt.ends_with(&expected_tail));
    }

    #[test]
    fn parser_continues_after_prefill() {
        let (encoding, _, prefill_tokens) = setup(vec![
            ChatMessage::user("Hi".into()),
            ChatMessage::assistant("Hello".into()),
        ]);
        let prefill_tokens = prefill_tokens.expect("missing prefill");
        let mut parser = StreamableParser::new(encoding.clone(), Some(Role::Assistant)).unwrap();
        for t in &prefill_tokens {
            parser.process(*t).unwrap();
        }
        let cont_tokens = encoding.tokenizer().encode_with_special_tokens(" world");
        for t in cont_tokens {
            parser.process(t).unwrap();
        }
        let delta = parser.last_content_delta().unwrap().unwrap();
        assert_eq!(delta, " world");
    }
}
