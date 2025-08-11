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

    pub fn add_step(&mut self, mut step: Node) -> usize {
        match &mut step {
            Node::Thought(t) => t.content_rev += 1,
            Node::Tool(t) => t.content_rev += 1,
            _ => {}
        }
        let block = self.ensure_last_assistant();
        block.steps.push(step);
        block.content_rev += 1;
        let idx = block.steps.len() - 1;
        self.dirty = true;
        idx
    }

    pub fn update_tool_result(&mut self, step_idx: usize, result: String) {
        let block = self.ensure_last_assistant();
        if let Some(Node::Tool(ToolStep {
            result: r,
            content_rev,
            ..
        })) = block.steps.get_mut(step_idx)
        {
            *r = result;
            *content_rev += 1;
            block.content_rev += 1;
            self.dirty = true;
        }
    }
}
