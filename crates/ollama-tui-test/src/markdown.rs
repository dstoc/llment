use crossterm::style::{Attribute, Color as CtColor};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use termimad::{
    Alignment, CompositeKind, CompoundStyle, FmtComposite, FmtLine, FmtTableRow, FmtText, MadSkin,
    RelativePosition, Spacing,
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
    if let Some(uc) = cs.object_style.underline_color {
        style = style.underline_color(map_color(uc));
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

fn composite_to_spans(skin: &MadSkin, fc: FmtComposite<'_>, width: usize) -> Vec<Span<'static>> {
    let mut spans = match fc.kind {
        CompositeKind::Code => {
            let ls = skin.line_style(fc.kind);
            let base_style = style_from_compound(&ls.compound_style);
            let (left_inner, right_inner) = fc.completions();
            let mut inner_width = left_inner + fc.visible_length + right_inner;
            if inner_width == 0 {
                inner_width = fc
                    .spacing
                    .map(|s| s.width)
                    .filter(|w| *w > 0)
                    .unwrap_or(width);
            }
            let (outer_left, outer_right) = if width > 0 {
                Spacing::optional_completions(skin.code_block.align, inner_width, Some(width))
            } else {
                (0, 0)
            };
            let mut spans: Vec<Span> = Vec::new();
            if outer_left > 0 {
                spans.push(Span::styled(" ".repeat(outer_left), base_style));
            }
            if fc.visible_length == 0 && left_inner + right_inner == 0 {
                spans.push(Span::styled(" ".repeat(inner_width), base_style));
            } else {
                if left_inner > 0 {
                    spans.push(Span::styled(" ".repeat(left_inner), base_style));
                }
                spans.extend(fc.compounds.into_iter().map(|c| {
                    let cs = skin.compound_style(ls, &c);
                    Span::styled(c.as_str().to_owned(), style_from_compound(&cs))
                }));
                if right_inner > 0 {
                    spans.push(Span::styled(" ".repeat(right_inner), base_style));
                }
            }
            if outer_right > 0 {
                spans.push(Span::styled(" ".repeat(outer_right), base_style));
            }
            spans
        }
        CompositeKind::Quote => {
            let ls = skin.line_style(fc.kind);
            let base_style = style_from_compound(&ls.compound_style);
            let (left, right) = fc.completions();
            let mut spans: Vec<Span> = Vec::new();
            let quote_style = style_from_compound(skin.quote_mark.compound_style());
            spans.push(Span::styled(
                format!("{} ", skin.quote_mark.get_char()),
                quote_style,
            ));
            if left > 0 {
                spans.push(Span::styled(" ".repeat(left), base_style));
            }
            spans.extend(fc.compounds.into_iter().map(|c| {
                let mut style = style_from_compound(&skin.compound_style(ls, &c));
                style = style.add_modifier(Modifier::ITALIC).fg(Color::Gray);
                Span::styled(c.as_str().to_owned(), style)
            }));
            if right > 0 {
                spans.push(Span::styled(" ".repeat(right), base_style));
            }
            spans
        }
        _ => {
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
    };
    spans.retain(|s| !s.content.is_empty());
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
    let mut skin = MadSkin::default();
    skin.table.align = Alignment::Center;
    skin.set_headers_fg(CtColor::AnsiValue(178));
    skin.bold.set_fg(CtColor::Yellow);
    skin.italic.set_fg(CtColor::Magenta);
    skin.code_block.align = Alignment::Center;
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
                out.push(Line::from(composite_to_spans(&skin, fc, width)));
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
                    spans.extend(composite_to_spans(&skin, cell, 0));
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
    use insta::assert_snapshot;
    use ratatui::{
        backend::TestBackend,
        buffer::Buffer,
        widgets::{Paragraph, Wrap},
        Terminal,
    };

    fn buffer_to_string(buffer: &Buffer) -> String {
        let area = buffer.area;
        let mut lines = Vec::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    fn render_markdown(md: &str, width: u16) -> String {
        let lines = markdown_to_lines(md, width as usize);
        let height = lines.len() as u16;
        if height == 0 {
            return String::new();
        }
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let p = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
                f.render_widget(p, f.area());
            })
            .unwrap();
        buffer_to_string(terminal.backend().buffer())
    }

    #[test]
    fn preserves_code_block_indentation() {
        let md = "```\nfunc foo() {\n   thing\n}\n```";
        let rendered = render_markdown(md, 80);
        assert_snapshot!(rendered, @r###"
                                  func foo() {
                                     thing
                                  }
"###);
    }

    #[test]
    fn renders_table_with_borders() {
        let md = "|a|b|\n|-|-|\n|1|2|";
        let rendered = render_markdown(md, 80);
        assert_snapshot!(rendered, @r###"
                                   ┌───┬───┐
                                   │ a │ b │
                                   ├───┼───┤
                                   │1  │2  │
                                   └───┴───┘
"###);
    }

    #[test]
    fn styles_block_quotes() {
        let md = "> quote";
        let rendered = render_markdown(md, 40);
        assert_snapshot!(rendered, @r###"
▐ quote
"###);
    }

    #[test]
    fn centers_code_block() {
        let md = "```\na\nbbbb\n```";
        let rendered = render_markdown(md, 10);
        assert_snapshot!(rendered, @r###"
   a
   bbbb
"###);
    }

    #[test]
    fn centers_table() {
        let table_md = "|a|b|\n|-|-|\n|1|2|";
        let table_rendered = render_markdown(table_md, 20);
        assert_snapshot!(table_rendered, @r###"
     ┌───┬───┐
     │ a │ b │
     ├───┼───┤
     │1  │2  │
     └───┴───┘
"###);
    }

    #[test]
    fn fills_blank_lines_in_code_block() {
        let md = "```\na\n\nb\n```";
        let rendered = render_markdown(md, 10);
        assert_snapshot!(rendered, @"    a     ");
    }

    #[test]
    fn styles_blank_only_code_block() {
        let md = "```\n\n```";
        let rendered = render_markdown(md, 10);
        assert_snapshot!(rendered, @"          ");
    }

    #[test]
    fn maps_inline_code_colors() {
        let md = "`code`";
        let rendered = render_markdown(md, 40);
        assert_snapshot!(rendered, @r###"
code
"###);
    }

    #[test]
    fn uses_custom_skin_colors() {
        let md = "# Head\n\n**bold** *italic*";
        let rendered = render_markdown(md, 80);
        assert_snapshot!(rendered, @r###"
Head

bold italic
"###);
    }

    #[test]
    fn renders_bulleted_list() {
        let md = "- one\n- two\n- three";
        let rendered = render_markdown(md, 20);
        assert_snapshot!(rendered, @r###"
- one
- two
- three
"###);
    }

    #[test]
    fn renders_numbered_list() {
        let md = "1. one\n2. two\n3. three";
        let rendered = render_markdown(md, 20);
        assert_snapshot!(rendered, @r###"
1. one
2. two
3. three
"###);
    }

    #[test]
    fn renders_horizontal_rule() {
        let md = "hello\n\n---\n\nworld";
        let rendered = render_markdown(md, 20);
        assert_snapshot!(rendered, @r###"
hello

――――――――――――――――――――

world
"###);
    }

    #[test]
    fn renders_strikethrough() {
        let md = "~~scratched~~";
        let rendered = render_markdown(md, 20);
        assert_snapshot!(rendered, @r###"
scratched
"###);
    }
}
