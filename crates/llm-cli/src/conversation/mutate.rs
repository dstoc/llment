use super::{
    Conversation, Node, ThoughtStep, UserBubble, assistant_block::AssistantBlock,
    tool_step::ToolStep,
};

impl Conversation {
    fn ensure_last_assistant(&mut self) -> &mut AssistantBlock {
        if !matches!(self.items.last(), Some(Node::Assistant(_))) {
            self.items.push(Node::Assistant(AssistantBlock::new(
                false,
                Vec::new(),
                String::new(),
            )));
        }
        match self.items.last_mut().unwrap() {
            Node::Assistant(block) => block,
            _ => unreachable!(),
        }
    }

    pub fn push_user(&mut self, text: String) {
        self.items.push(Node::User(UserBubble::new(text)));
        self.dirty = true;
        self.ensure_layout(self.width);
        self.scroll_to_bottom();
    }

    pub fn push_assistant_block(&mut self) {
        let at_bottom = self.is_at_bottom();
        self.items.push(Node::Assistant(AssistantBlock::new(
            false,
            Vec::new(),
            String::new(),
        )));
        self.dirty = true;
        self.ensure_layout(self.width);
        if at_bottom {
            self.scroll_to_bottom();
        }
    }

    pub fn append_thinking(&mut self, text: &str) {
        let at_bottom = self.is_at_bottom();
        let block = self.ensure_last_assistant();
        block.record_activity();
        if let Some(Node::Thought(t)) = block.steps.last_mut() {
            t.text.push_str(text);
            t.content_rev += 1;
        } else {
            block
                .steps
                .push(Node::Thought(ThoughtStep::new(text.into())));
        }
        block.content_rev += 1;
        self.dirty = true;
        self.ensure_layout(self.width);
        if at_bottom {
            self.scroll_to_bottom();
        }
    }

    pub fn append_response(&mut self, text: &str) {
        let at_bottom = self.is_at_bottom();
        let block = self.ensure_last_assistant();
        block.record_activity();
        block.response.push_str(text);
        block.content_rev += 1;
        self.dirty = true;
        self.ensure_layout(self.width);
        if at_bottom {
            self.scroll_to_bottom();
        }
    }

    pub fn add_step(&mut self, mut step: Node) -> usize {
        let at_bottom = self.is_at_bottom();
        match &mut step {
            Node::Thought(t) => t.content_rev += 1,
            Node::Tool(t) => t.content_rev += 1,
            _ => {}
        }
        let block = self.ensure_last_assistant();
        block.record_activity();
        block.steps.push(step);
        block.content_rev += 1;
        let idx = block.steps.len() - 1;
        self.dirty = true;
        self.ensure_layout(self.width);
        if at_bottom {
            self.scroll_to_bottom();
        }
        idx
    }

    pub fn update_tool_result(&mut self, step_idx: usize, result: String, failed: bool) {
        let at_bottom = self.is_at_bottom();
        let block = self.ensure_last_assistant();
        if let Some(Node::Tool(ToolStep {
            result: r,
            done,
            failed: f,
            content_rev,
            ..
        })) = block.steps.get_mut(step_idx)
        {
            *r = result;
            *done = true;
            *f = failed;
            *content_rev += 1;
            block.record_activity();
            block.content_rev += 1;
            self.dirty = true;
            self.ensure_layout(self.width);
            if at_bottom {
                self.scroll_to_bottom();
            }
        }
    }

    pub fn redo_last(&mut self) -> Option<String> {
        if matches!(self.items.last(), Some(Node::Assistant(_))) {
            self.items.pop();
            if let Some(Node::User(user)) = self.items.pop() {
                self.dirty = true;
                self.ensure_layout(self.width);
                self.scroll_to_bottom();
                return Some(user.text);
            }
        }
        None
    }
}
