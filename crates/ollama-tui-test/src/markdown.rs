use crossterm::style::{Attribute, Color as CtColor};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use termimad::{CompoundStyle, FmtComposite, FmtLine, FmtText, MadSkin, Spacing};

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

fn style_from_compound(cs: &CompoundStyle) -> Style {
    let mut style = Style::default();
    if let Some(fg) = cs.object_style.foreground_color {
        style = style.fg(map_color(fg));
    }
    if let Some(bg) = cs.object_style.background_color {
        style = style.bg(map_color(bg));
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
    if attrs.has(Attribute::Reverse) {
        style = style.add_modifier(Modifier::REVERSED);
    }
    if attrs.has(Attribute::Dim) {
        style = style.add_modifier(Modifier::DIM);
    }
    style
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
        Span::styled(c.as_str().to_owned(), style_from_compound(&cs))
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
            FmtLine::Normal(fc) => Line::from(composite_to_spans(&skin, fc)),
            FmtLine::TableRow(row) => {
                let tbc = skin.table_border_chars;
                let border_style = style_from_compound(&skin.table.compound_style);
                let tbl_width = 1 + row.cells.iter().fold(0, |sum, cell| {
                    sum + cell.spacing.map(|s| s.width).unwrap_or(cell.visible_length) + 1
                });
                let (lpo, rpo) =
                    Spacing::optional_completions(skin.table.align, tbl_width, Some(width));
                let mut spans = Vec::new();
                if lpo > 0 {
                    spans.push(Span::raw(" ".repeat(lpo)));
                }
                spans.push(Span::styled(tbc.vertical.to_string(), border_style));
                for cell in row.cells {
                    spans.extend(composite_to_spans(&skin, cell));
                    spans.push(Span::styled(tbc.vertical.to_string(), border_style));
                }
                if rpo > 0 {
                    spans.push(Span::raw(" ".repeat(rpo)));
                }
                Line::from(spans)
            }
            FmtLine::TableRule(rule) => {
                let tbc = skin.table_border_chars;
                let border_style = style_from_compound(&skin.table.compound_style);
                let tbl_width = 1 + rule.widths.iter().fold(0, |sum, w| sum + w + 1);
                let (lpo, rpo) =
                    Spacing::optional_completions(skin.table.align, tbl_width, Some(width));
                let mut spans = Vec::new();
                if lpo > 0 {
                    spans.push(Span::raw(" ".repeat(lpo)));
                }
                let left = match rule.position {
                    termimad::RelativePosition::Top => tbc.top_left_corner,
                    termimad::RelativePosition::Other => tbc.left_junction,
                    termimad::RelativePosition::Bottom => tbc.bottom_left_corner,
                };
                let junction = match rule.position {
                    termimad::RelativePosition::Top => tbc.top_junction,
                    termimad::RelativePosition::Other => tbc.cross,
                    termimad::RelativePosition::Bottom => tbc.bottom_junction,
                };
                let right = match rule.position {
                    termimad::RelativePosition::Top => tbc.top_right_corner,
                    termimad::RelativePosition::Other => tbc.right_junction,
                    termimad::RelativePosition::Bottom => tbc.bottom_right_corner,
                };
                spans.push(Span::styled(left.to_string(), border_style));
                for (idx, w) in rule.widths.iter().enumerate() {
                    spans.push(Span::styled(
                        tbc.horizontal.to_string().repeat(*w),
                        border_style,
                    ));
                    if idx + 1 < rule.widths.len() {
                        spans.push(Span::styled(junction.to_string(), border_style));
                    }
                }
                spans.push(Span::styled(right.to_string(), border_style));
                if rpo > 0 {
                    spans.push(Span::raw(" ".repeat(rpo)));
                }
                Line::from(spans)
            }
            FmtLine::HorizontalRule => {
                let hr_style = style_from_compound(skin.horizontal_rule.compound_style());
                let ch = skin.horizontal_rule.get_char();
                Line::from(vec![Span::styled(ch.to_string().repeat(width), hr_style)])
            }
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
                .any(|l| l.spans.iter().any(|s| s.content.contains("fn main()")))
        );
    }

    #[test]
    fn renders_table() {
        let md = "|a|b|\n|-|-|\n|1|2|";
        let text = markdown_to_lines(md, 80);
        let row = text
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("a")))
            .unwrap();
        let row_str: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(row_str.contains("│ a │ b │"));
        let rule = text
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("┼")))
            .unwrap();
        let rule_str: String = rule.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(rule_str.contains("├───┼───┤"));
    }
}
