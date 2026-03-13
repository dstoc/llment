use llm::{AssistantPart, ChatMessage};

/// Result of applying a history edit.
#[derive(Default)]
pub struct HistoryEditResult {
    /// Optional prompt text to set in the input.
    pub prompt: Option<String>,
    /// Whether session token counters should be reset.
    pub reset_session: bool,
    /// Whether any in-flight requests should be aborted.
    pub abort_requests: bool,
}

/// Callback mutating chat history and returning a `HistoryEditResult` or error string.
pub type HistoryEdit =
    Box<dyn FnOnce(&mut Vec<ChatMessage>) -> Result<HistoryEditResult, String> + Send>;

pub fn append_thought(text: String) -> HistoryEdit {
    Box::new(move |history: &mut Vec<ChatMessage>| {
        history.push(ChatMessage::Assistant(llm::AssistantMessage {
            content: vec![AssistantPart::Thinking {
                text: text.clone(),
                encrypted_content: None,
            }],
        }));
        Ok(HistoryEditResult::default())
    })
}

pub fn append_response(text: String) -> HistoryEdit {
    Box::new(move |history: &mut Vec<ChatMessage>| {
        let append = matches!(
            history.last(),
            Some(ChatMessage::Assistant(a))
                if !a
                    .content
                    .iter()
                    .any(|p| matches!(p, AssistantPart::Text { .. } | AssistantPart::ToolCall { .. }))
        );
        if append {
            if let Some(ChatMessage::Assistant(a)) = history.last_mut() {
                a.content.push(AssistantPart::Text {
                    text: text.clone(),
                    encrypted_content: None,
                });
            }
        } else {
            history.push(ChatMessage::assistant(text.clone()));
        }
        Ok(HistoryEditResult::default())
    })
}

pub fn pop() -> HistoryEdit {
    Box::new(|history: &mut Vec<ChatMessage>| {
        let mut removed = false;
        if let Some(ChatMessage::Assistant(a)) = history.last_mut() {
            if a.content.pop().is_some() {
                removed = true;
                if a.content.is_empty() {
                    history.pop();
                }
            }
        }
        if !removed {
            history.pop();
        }
        Ok(HistoryEditResult::default())
    })
}

pub fn clear() -> HistoryEdit {
    Box::new(|history: &mut Vec<ChatMessage>| {
        history.clear();
        Ok(HistoryEditResult {
            reset_session: true,
            abort_requests: true,
            ..HistoryEditResult::default()
        })
    })
}

pub fn redo() -> HistoryEdit {
    Box::new(|history: &mut Vec<ChatMessage>| {
        let mut prompt = None;
        while let Some(msg) = history.pop() {
            if let ChatMessage::User(u) = msg {
                prompt = Some(u.content);
                break;
            }
        }
        Ok(HistoryEditResult {
            prompt,
            abort_requests: true,
            ..HistoryEditResult::default()
        })
    })
}

pub fn save(path: String) -> HistoryEdit {
    Box::new(move |history: &mut Vec<ChatMessage>| {
        let mut lines = Vec::new();
        for msg in history.iter() {
            if let ChatMessage::Assistant(a) = msg {
                for part in &a.content {
                    let single_part_msg = ChatMessage::Assistant(llm::AssistantMessage {
                        content: vec![part.clone()],
                    });
                    lines.push(serde_json::to_string(&single_part_msg).unwrap());
                }
            } else {
                lines.push(serde_json::to_string(msg).unwrap());
            }
        }

        let data = lines.join("\n") + "\n";
        std::fs::write(&path, data).map_err(|e| format!("failed to save: {}", e))?;
        Ok(HistoryEditResult::default())
    })
}

pub fn load(path: String) -> HistoryEdit {
    Box::new(move |history: &mut Vec<ChatMessage>| {
        let data = std::fs::read_to_string(&path).map_err(|e| format!("failed to load: {}", e))?;

        let mut loaded: Vec<ChatMessage> = Vec::new();
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let msg: ChatMessage =
                serde_json::from_str(line).map_err(|e| format!("failed to parse: {}", e))?;

            if let ChatMessage::Assistant(mut new_a) = msg {
                if let Some(ChatMessage::Assistant(existing_a)) = loaded.last_mut() {
                    existing_a.content.append(&mut new_a.content);
                } else {
                    loaded.push(ChatMessage::Assistant(new_a));
                }
            } else {
                loaded.push(msg);
            }
        }

        *history = loaded;
        Ok(HistoryEditResult {
            reset_session: true,
            abort_requests: true,
            ..HistoryEditResult::default()
        })
    })
}
