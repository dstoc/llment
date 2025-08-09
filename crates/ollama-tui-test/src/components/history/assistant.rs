#[derive(Clone)]
pub struct AssistantItem(pub String);

impl AssistantItem {
    pub fn render(&self) -> Vec<(String, bool, bool)> {
        vec![(self.0.clone(), true, false)]
    }
}
