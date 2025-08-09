#[derive(Clone)]
pub struct SeparatorItem;

impl SeparatorItem {
    pub fn render(&self) -> Vec<(String, bool, bool)> {
        vec![(String::new(), false, false)]
    }
}
