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
        Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, widgets::Paragraph,
    };

    fn render_markdown(md: &str, width: u16) -> Buffer {
        let lines = markdown_to_lines(md, width as usize);
        let height = lines.len() as u16;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let paragraph = Paragraph::new(lines.clone());
                f.render_widget(paragraph, Rect::new(0, 0, width, height));
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_to_debug_string(buf: &Buffer) -> String {
        let mut out = String::new();
        for y in buf.area.top()..buf.area.bottom() {
            let mut prev_fg = None;
            let mut prev_bg = None;
            for x in buf.area.left()..buf.area.right() {
                let c = buf.cell((x, y)).unwrap();
                let fg = c.style().fg;
                let bg = c.style().bg;
                if prev_fg != fg || prev_bg != bg {
                    let fg_str = fg.map(|c| format!("{:?}", c)).unwrap_or_else(|| "_".into());
                    let bg_str = bg.map(|c| format!("{:?}", c)).unwrap_or_else(|| "_".into());
                    out.push_str(&format!("[{},{}]", fg_str, bg_str));
                    prev_fg = fg;
                    prev_bg = bg;
                }
                out.push_str(c.symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn preserves_code_block_indentation() {
        let md = "```
func foo() {
   thing
}
```";
        let buffer = render_markdown(md, 20);
        let dbg = buffer_to_debug_string(&buffer);
        assert_snapshot!(dbg, @r"
        [Indexed(249),Indexed(235)]    func foo() {    
        [Indexed(249),Indexed(235)]       thing        
        [Indexed(249),Indexed(235)]    }
        ");
    }

    #[test]
    fn renders_table_with_borders() {
        let md = "|a|b|\n|-|-|\n|1|2|";
        let buffer = render_markdown(md, 20);
        let dbg = buffer_to_debug_string(&buffer);
        assert_snapshot!(dbg, @r"
        [Reset,Reset]     [Indexed(239),Reset]┌───┬───┐[Reset,Reset]      
        [Reset,Reset]     [Indexed(239),Reset]│[Reset,Reset] a [Indexed(239),Reset]│[Reset,Reset] b [Indexed(239),Reset]│[Reset,Reset]      
        [Reset,Reset]     [Indexed(239),Reset]├───┼───┤[Reset,Reset]      
        [Reset,Reset]     [Indexed(239),Reset]│[Reset,Reset]1  [Indexed(239),Reset]│[Reset,Reset]2  [Indexed(239),Reset]│[Reset,Reset]      
        [Reset,Reset]     [Indexed(239),Reset]└───┴───┘[Reset,Reset]
        ");
    }

    #[test]
    fn styles_block_quotes() {
        let buffer = render_markdown("> quote", 20);
        let dbg = buffer_to_debug_string(&buffer);
        assert_snapshot!(dbg, @"[Indexed(244),Reset]▐ [Gray,Reset]quote[Reset,Reset]");
    }

    #[test]
    fn centers_code_block() {
        let buffer = render_markdown("```\na\nbbbb\n```", 10);
        let dbg = buffer_to_debug_string(&buffer);
        assert_snapshot!(dbg, @r"
        [Indexed(249),Indexed(235)]   a      
        [Indexed(249),Indexed(235)]   bbbb
        ");
    }

    #[test]
    fn centers_table() {
        let buffer = render_markdown("|a|b|\n|-|-|\n|1|2|", 20);
        let dbg = buffer_to_debug_string(&buffer);
        assert_snapshot!(dbg, @r"
        [Reset,Reset]     [Indexed(239),Reset]┌───┬───┐[Reset,Reset]      
        [Reset,Reset]     [Indexed(239),Reset]│[Reset,Reset] a [Indexed(239),Reset]│[Reset,Reset] b [Indexed(239),Reset]│[Reset,Reset]      
        [Reset,Reset]     [Indexed(239),Reset]├───┼───┤[Reset,Reset]      
        [Reset,Reset]     [Indexed(239),Reset]│[Reset,Reset]1  [Indexed(239),Reset]│[Reset,Reset]2  [Indexed(239),Reset]│[Reset,Reset]      
        [Reset,Reset]     [Indexed(239),Reset]└───┴───┘[Reset,Reset]
        ");
    }

    #[test]
    fn fills_blank_lines_in_code_block() {
        let buffer = render_markdown("```\na\n\nb\n```", 10);
        let dbg = buffer_to_debug_string(&buffer);
        assert_snapshot!(dbg, @r"
        [Indexed(249),Indexed(235)]    a     
        [Indexed(249),Indexed(235)]          
        [Indexed(249),Indexed(235)]    b
        ");
    }

    #[test]
    fn styles_blank_only_code_block() {
        let buffer = render_markdown("```\n\n```", 10);
        let dbg = buffer_to_debug_string(&buffer);
        assert_snapshot!(dbg, @"[Indexed(249),Indexed(235)]");
    }

    #[test]
    fn maps_inline_code_colors() {
        let buffer = render_markdown("`code`", 40);
        let dbg = buffer_to_debug_string(&buffer);
        assert_snapshot!(dbg, @"[Indexed(249),Indexed(235)]code[Reset,Reset]");
    }

    #[test]
    fn uses_custom_skin_colors() {
        let buffer = render_markdown("# Head\n\n**bold** *italic*", 40);
        let dbg = buffer_to_debug_string(&buffer);
        assert_snapshot!(dbg, @r"
        [Indexed(178),Reset]Head[Reset,Reset]                                    
        [Reset,Reset]                                        
        [LightYellow,Reset]bold[Reset,Reset] [LightMagenta,Reset]italic[Reset,Reset]
        ");
    }
}
