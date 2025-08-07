use crossterm::style::{Attribute, Color as CtColor};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use termimad::{FmtComposite, FmtText, MadSkin};

fn map_color(color: CtColor) -> Color {
    match color {
        CtColor::Black => Color::Black,
        CtColor::DarkGrey => Color::DarkGray,
        CtColor::Red => Color::LightRed,
        CtColor::DarkRed => Color::Red,
        CtColor::Green => Color::LightGreen,
        CtColor::DarkGreen => Color::Green,
        CtColor::Yellow => Color::LightYellow,
        CtColor::DarkYellow => Color::Yellow,
        CtColor::Blue => Color::LightBlue,
        CtColor::DarkBlue => Color::Blue,
        CtColor::Magenta => Color::LightMagenta,
        CtColor::DarkMagenta => Color::Magenta,
        CtColor::Cyan => Color::LightCyan,
        CtColor::DarkCyan => Color::Cyan,
        CtColor::White => Color::White,
        CtColor::Grey => Color::Gray,
        CtColor::Rgb { r, g, b } => Color::Rgb(r, g, b),
        CtColor::AnsiValue(v) => Color::Indexed(v),
        CtColor::Reset => Color::Reset,
    }
}

fn composite_to_spans(skin: &MadSkin, fc: FmtComposite<'_>) -> Vec<Span<'static>> {
    let ls = skin.line_style(fc.kind);
    let (left, right) = fc.completions();
    let mut spans: Vec<Span> = Vec::new();
    if left > 0 {
        spans.push(Span::raw(" ".repeat(left)));
    }
    spans.extend(fc.compounds.into_iter().map(|c| {
        let cs = skin.compound_style(ls, &c);
        let mut style = Style::default();
        if let Some(fg) = cs.object_style.foreground_color {
            style = style.fg(map_color(fg));
        }
        let attrs = cs.object_style.attributes;
        if attrs.has(Attribute::Bold) {
            style = style.add_modifier(Modifier::BOLD);
        }
        if attrs.has(Attribute::Italic) {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if attrs.has(Attribute::Underlined) {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        if attrs.has(Attribute::CrossedOut) {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        Span::styled(c.as_str().to_owned(), style)
    }));
    if right > 0 {
        spans.push(Span::raw(" ".repeat(right)));
    }
    spans
}

pub fn markdown_to_lines(md: &str, width: usize) -> Vec<Line<'static>> {
    let skin = MadSkin::default();
    let fmt = FmtText::from(&skin, md, Some(width));
    fmt.lines
        .into_iter()
        .map(|line| match line {
            termimad::FmtLine::Normal(fc) => Line::from(composite_to_spans(&skin, fc)),
            termimad::FmtLine::TableRow(row) => {
                let mut spans = Vec::new();
                spans.push(Span::raw("|"));
                for cell in row.cells {
                    spans.extend(composite_to_spans(&skin, cell));
                    spans.push(Span::raw("|"));
                }
                Line::from(spans)
            }
            termimad::FmtLine::TableRule(rule) => {
                let mut spans = Vec::new();
                spans.push(Span::raw("+"));
                for w in rule.widths {
                    spans.push(Span::raw("-".repeat(w)));
                    spans.push(Span::raw("+"));
                }
                Line::from(spans)
            }
            termimad::FmtLine::HorizontalRule => Line::raw("-".repeat(width)),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_code_block() {
        let md = "```\nfn main() {}\n```";
        let text = markdown_to_lines(md, 80);
        assert!(
            text.iter()
                .any(|l| { l.spans.iter().any(|s| s.content.contains("fn main()")) })
        );
    }

    #[test]
    fn renders_table() {
        let md = "|a|b|\n|-|-|\n|1|2|";
        let text = markdown_to_lines(md, 80);
        assert!(
            text.iter()
                .any(|l| l.spans.iter().any(|s| s.content.contains("|")))
        );
        assert!(
            text.iter()
                .any(|l| l.spans.iter().any(|s| s.content.contains("a")))
        );
        assert!(
            text.iter()
                .any(|l| l.spans.iter().any(|s| s.content.contains("1")))
        );
    }
}
