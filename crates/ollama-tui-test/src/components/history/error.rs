#[derive(Clone)]
pub struct ErrorItem(pub String);

impl ErrorItem {
    pub fn render(&self) -> Vec<(String, bool, bool)> {
        vec![(self.0.clone(), false, true)]
    }
}
