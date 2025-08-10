use super::{Conversation, Node, ThoughtStep, UserBubble, assistant_block::AssistantBlock};

#[allow(dead_code)]
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
    }

    pub fn push_assistant_block(&mut self) {
        self.items.push(Node::Assistant(AssistantBlock::new(
            false,
            Vec::new(),
            String::new(),
        )));
        self.dirty = true;
    }

    pub fn append_thinking(&mut self, text: &str) {
        let block = self.ensure_last_assistant();
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
    }

    pub fn append_response(&mut self, text: &str) {
        let block = self.ensure_last_assistant();
        block.response.push_str(text);
        block.content_rev += 1;
        self.dirty = true;
    }

    pub fn add_step(&mut self, mut step: Node) {
        match &mut step {
            Node::Thought(t) => t.content_rev += 1,
            Node::Tool(t) => t.content_rev += 1,
            _ => {}
        }
        let block = self.ensure_last_assistant();
        block.steps.push(step);
        block.content_rev += 1;
        self.dirty = true;
    }
}
