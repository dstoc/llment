use std::time::{Duration, Instant};
use textwrap::wrap;

#[derive(Clone)]
pub enum ThinkingStep {
    Thought(String),
    ToolCall {
        name: String,
        args: String,
        result: String,
        success: bool,
        collapsed: bool,
    },
}

#[derive(Clone)]
pub struct ThinkingItem {
    pub steps: Vec<ThinkingStep>,
    pub collapsed: bool,
    pub start: Instant,
    pub duration: Duration,
    pub done: bool,
}

impl ThinkingItem {
    pub fn render(&self, width: usize) -> Vec<(String, bool, bool)> {
        let mut lines = Vec::new();
        let calls = self
            .steps
            .iter()
            .filter(|s| matches!(s, ThinkingStep::ToolCall { .. }))
            .count();
        if self.done {
            let summary = format!(
                "Thought for {} seconds, {calls} tool call{}",
                self.duration.as_secs(),
                if calls == 1 { "" } else { "s" },
            );
            let arrow = if self.collapsed { "›" } else { "⌄" };
            lines.push((format!("{summary} {arrow}"), false, false));
        } else {
            lines.push(("Thinking ⌄".to_string(), false, false));
        }
        if !self.collapsed || !self.done {
            for step in &self.steps {
                match step {
                    ThinkingStep::Thought(t) => {
                        let wrapped = wrap(t, width.saturating_sub(2).max(1));
                        for w in wrapped {
                            lines.push((format!("· {}", w), false, false));
                        }
                    }
                    ThinkingStep::ToolCall {
                        name,
                        args,
                        result,
                        success,
                        collapsed,
                    } => {
                        let arrow = if *collapsed { "›" } else { "⌄" };
                        lines.push((format!("· _{name}_ {arrow}"), true, !*success));
                        if !*collapsed {
                            lines.push((format!("  args: {args}"), false, false));
                            lines.push((format!("  result: {result}"), false, false));
                        }
                    }
                }
            }
        }
        lines.push((String::new(), false, false));
        lines
    }
}
