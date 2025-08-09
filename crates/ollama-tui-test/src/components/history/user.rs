use textwrap::wrap;

#[derive(Clone)]
pub struct UserItem(pub String);

impl UserItem {
    pub fn render(&self, width: usize) -> Vec<(String, bool, bool)> {
        let inner_width = width.saturating_sub(7);
        let wrapped = wrap(&self.0, inner_width.max(1));
        let box_width = wrapped.iter().map(|l| l.len()).max().unwrap_or(0);
        let mut lines = Vec::new();
        lines.push((format!("     ┌{}┐", "─".repeat(box_width)), false, false));
        for w in wrapped {
            let mut line = w.into_owned();
            line.push_str(&" ".repeat(box_width.saturating_sub(line.len())));
            lines.push((format!("     │{}│", line), false, false));
        }
        lines.push((format!("     └{}┘", "─".repeat(box_width)), false, false));
        lines.push((String::new(), false, false));
        lines
    }
}
