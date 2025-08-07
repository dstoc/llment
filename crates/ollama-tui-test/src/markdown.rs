use crossterm::style::{Attribute, Color as CtColor};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use termimad::{
    CompoundStyle, FmtComposite, FmtLine, FmtTableRow, FmtText, MadSkin, RelativePosition, Spacing,
};

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

fn render_table_rule(
    skin: &MadSkin,
    widths: &[usize],
    pos: RelativePosition,
    width: usize,
) -> Line<'static> {
    let tbc = skin.table_border_chars;
    let border_style = style_from_compound(&skin.table.compound_style);
    let tbl_width = 1 + widths.iter().fold(0, |sum, w| sum + w + 1);
    let (lpo, rpo) = Spacing::optional_completions(skin.table.align, tbl_width, Some(width));
    let mut spans = Vec::new();
    if lpo > 0 {
        spans.push(Span::raw(" ".repeat(lpo)));
    }
    let left = match pos {
        RelativePosition::Top => tbc.top_left_corner,
        RelativePosition::Other => tbc.left_junction,
        RelativePosition::Bottom => tbc.bottom_left_corner,
    };
    let junction = match pos {
        RelativePosition::Top => tbc.top_junction,
        RelativePosition::Other => tbc.cross,
        RelativePosition::Bottom => tbc.bottom_junction,
    };
    let right = match pos {
        RelativePosition::Top => tbc.top_right_corner,
        RelativePosition::Other => tbc.right_junction,
        RelativePosition::Bottom => tbc.bottom_right_corner,
    };
    spans.push(Span::styled(left.to_string(), border_style));
    for (idx, w) in widths.iter().enumerate() {
        spans.push(Span::styled(
            tbc.horizontal.to_string().repeat(*w),
            border_style,
        ));
        if idx + 1 < widths.len() {
            spans.push(Span::styled(junction.to_string(), border_style));
        }
    }
    spans.push(Span::styled(right.to_string(), border_style));
    if rpo > 0 {
        spans.push(Span::raw(" ".repeat(rpo)));
    }
    Line::from(spans)
}

pub fn markdown_to_lines(md: &str, width: usize) -> Vec<Line<'static>> {
    let skin = MadSkin::default();
    let fmt = FmtText::from(&skin, md, Some(width));
    let mut out: Vec<Line> = Vec::new();
    let mut current_table: Option<Vec<usize>> = None;
    for line in fmt.lines {
        match line {
            FmtLine::Normal(fc) => {
                if let Some(widths) = current_table.take() {
                    out.push(render_table_rule(
                        &skin,
                        &widths,
                        RelativePosition::Bottom,
                        width,
                    ));
                }
                out.push(Line::from(composite_to_spans(&skin, fc)));
            }
            FmtLine::TableRow(FmtTableRow { cells }) => {
                let widths: Vec<usize> = cells
                    .iter()
                    .map(|c| c.spacing.map(|s| s.width).unwrap_or(c.visible_length))
                    .collect();
                if current_table.is_none() {
                    out.push(render_table_rule(
                        &skin,
                        &widths,
                        RelativePosition::Top,
                        width,
                    ));
                }
                current_table = Some(widths.clone());
                let tbc = skin.table_border_chars;
                let border_style = style_from_compound(&skin.table.compound_style);
                let tbl_width = 1 + widths.iter().fold(0, |sum, w| sum + w + 1);
                let (lpo, rpo) =
                    Spacing::optional_completions(skin.table.align, tbl_width, Some(width));
                let mut spans = Vec::new();
                if lpo > 0 {
                    spans.push(Span::raw(" ".repeat(lpo)));
                }
                spans.push(Span::styled(tbc.vertical.to_string(), border_style));
                for cell in cells {
                    spans.extend(composite_to_spans(&skin, cell));
                    spans.push(Span::styled(tbc.vertical.to_string(), border_style));
                }
                if rpo > 0 {
                    spans.push(Span::raw(" ".repeat(rpo)));
                }
                out.push(Line::from(spans));
            }
            FmtLine::TableRule(rule) => {
                out.push(render_table_rule(
                    &skin,
                    &rule.widths,
                    RelativePosition::Other,
                    width,
                ));
            }
            FmtLine::HorizontalRule => {
                if let Some(widths) = current_table.take() {
                    out.push(render_table_rule(
                        &skin,
                        &widths,
                        RelativePosition::Bottom,
                        width,
                    ));
                }
                let hr_style = style_from_compound(skin.horizontal_rule.compound_style());
                let ch = skin.horizontal_rule.get_char();
                out.push(Line::from(vec![Span::styled(
                    ch.to_string().repeat(width),
                    hr_style,
                )]));
            }
        }
    }
    if let Some(widths) = current_table.take() {
        out.push(render_table_rule(
            &skin,
            &widths,
            RelativePosition::Bottom,
            width,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_code_block_indentation() {
        let md = "```
func foo() {
   thing
}
```";
        let text = markdown_to_lines(md, 80);
        assert!(
            text.iter()
                .any(|l| l.spans.iter().any(|s| s.content.contains("   thing")))
        );
    }

    #[test]
    fn renders_table_with_borders() {
        let md = "|a|b|\n|-|-|\n|1|2|";
        let text = markdown_to_lines(md, 80);
        let top = text.first().unwrap();
        let top_str: String = top.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(top_str.contains("┌"));
        let row = text
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("a")))
            .unwrap();
        let row_str: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(row_str.contains("│ a │ b │"));
        let bottom = text.last().unwrap();
        let bottom_str: String = bottom.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(bottom_str.contains("└"));
    }
}
