//! Code colors and authored font semantics are independent presentation layers.

use super::*;

const ROFF: &str = r#".TH HIGHLIGHT 3 "2026-10-15" "ManT" "Library Functions"
.SH NAME
highlight \- syntax coloring probe
.SH SYNOPSIS
.nf
.B #include <regex.h>
.P
.BI "int regcomp(regex_t *restrict " preg ", const char *restrict " regex ,
.BI "            int " cflags ");"
.P
.B "const char *message = \[dq]text\[dq]; /* note */"
.B typedef struct {
.BI "    size_t " re_nsub ";"
.B } regex_t;
.BR typedef " /* ... */ " regoff_t;
.fi
.SH DESCRIPTION
Probe.
"#;

fn style_at(rendered: &RenderedDocument, needle: &str) -> Style {
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

    let hit = &rendered.search(needle)[0];
    let row = &rendered.text.lines[hit.row];
    let area = Rect::new(0, 0, 240, 1);
    let mut buffer = Buffer::empty(area);
    row.clone().render(area, &mut buffer);
    buffer[(u16::try_from(hit.start_column).unwrap(), 0)].style()
}

#[test]
fn native_bold_c_synopsis_keeps_syntax_colors_and_original_fonts() {
    // This exact source ran fixed CVS ASCII/UTF-8/HTML/tree/lint first, lint=0.
    // man_term.c::pre_B and pre_alternate(BI/BR) select fonts; print_man_node
    // retains no-fill rows. Preserve those fonts and text, not a native color.
    let content = mant_loader::load_roff_bytes(ROFF.as_bytes()).unwrap();
    let before = content.clone();
    let view = DocumentView::new(&content);
    for width in [40, 120, 240] {
        let rendered = view.render(width);
        for (needle, foreground) in [
            ("#include", theme::MAUVE),
            ("int regcomp", theme::MAUVE),
            ("const char", theme::MAUVE),
            ("typedef struct", theme::MAUVE),
            ("struct {", theme::MAUVE),
            ("(regex_t", theme::MAUVE),
            ("{", theme::MAUVE),
            ("} regex_t", theme::MAUVE),
            ("=", theme::BLUE),
            ("\"text\"", theme::BLUE),
            ("/* note */", theme::SUBTEXT),
        ] {
            let style = style_at(&rendered, needle);
            assert_eq!(style.fg, Some(foreground), "{needle} at {width}");
            assert_eq!(style.bg, Some(theme::SURFACE), "{needle} at {width}");
            assert!(style.add_modifier.contains(Modifier::BOLD), "{needle}");
        }
        assert!(
            style_at(&rendered, "preg")
                .add_modifier
                .contains(Modifier::ITALIC)
        );
        assert!(
            style_at(&rendered, "/* note */")
                .add_modifier
                .contains(Modifier::ITALIC)
        );
        assert_eq!(style_at(&rendered, "regcomp").fg, Some(theme::STRONG));
        assert_eq!(style_at(&rendered, "/* ... */").fg, Some(theme::SUBTEXT));
        assert_eq!(rendered.search("regcomp(regex_t *restrict preg").len(), 1);
        assert_eq!(rendered.text, view.render(width).text);
    }
    assert_eq!(content, before, "presentation must not rewrite IR");
}

#[test]
fn lexical_colors_cross_nested_fonts_without_changing_prose_rules() {
    let nodes = vec![Inline::Strong {
        children: vec![Inline::Emphasis {
            children: vec![Inline::Code {
                value: "return unknown_token + 12; /* note */".into(),
            }],
        }],
    }];
    let render = |code| {
        styled_reference_inline_lines(
            &nodes,
            theme::style(theme::StyleRole::Text),
            None,
            crate::document::entries::InlineStyles::default(),
            code,
            &HashMap::default(),
        )
    };
    let lines = render(true);
    for span in &lines[0].spans {
        assert!(
            span.style
                .add_modifier
                .contains(Modifier::BOLD | Modifier::ITALIC)
        );
        assert_eq!(span.style.bg, Some(theme::SURFACE));
    }
    assert_eq!(lines[0].spans[0].style.fg, Some(theme::MAUVE));
    assert_eq!(
        lines[0]
            .spans
            .iter()
            .find(|span| span.content.contains("unknown_token"))
            .unwrap()
            .style
            .fg,
        Some(theme::STRONG)
    );
    assert_eq!(
        lines[0]
            .spans
            .iter()
            .find(|span| span.content.contains("12"))
            .unwrap()
            .style
            .fg,
        Some(theme::YELLOW)
    );
    for span in &render(false)[0].spans {
        assert_eq!(span.style.fg, Some(theme::SUBTEXT_BRIGHT));
        assert!(
            span.style
                .add_modifier
                .contains(Modifier::BOLD | Modifier::ITALIC)
        );
    }
}
