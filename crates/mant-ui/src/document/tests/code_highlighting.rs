//! Syntax coloring consumes unchanged source-neutral inline text before reflow.

use super::*;
use crate::document::inline::styled_reference_inline_lines;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

#[test]
fn code_state_crosses_authored_styles_links_anchors_and_structural_breaks() {
    let nodes = vec![
        text("/* "),
        Inline::Strong {
            children: vec![Inline::Emphasis {
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::Section {
                        id: "target".into(),
                    },
                    title: None,
                    children: vec![Inline::Code {
                        value: "变量".into(),
                    }],
                }],
            }],
        },
        Inline::LineBreak { indent_columns: 7 },
        Inline::anchor("middle"),
        text("return 12; */\nreturn 34;"),
    ];
    let names = [mant_render::InlineNameRange {
        chars: 3..5,
        kind: EntryKind::Variable,
    }];
    let lines = styled_reference_inline_lines(
        &nodes,
        theme::style(theme::StyleRole::Text),
        None,
        &names,
        true,
        &HashMap::default(),
    );
    assert_eq!(lines.len(), 3);
    let bound = lines[0]
        .spans
        .iter()
        .find(|span| span.content == "变量")
        .unwrap();
    assert_eq!(bound.style.fg, Some(theme::PINK));
    assert_eq!(bound.style.bg, Some(theme::SURFACE));
    assert!(
        bound
            .style
            .add_modifier
            .contains(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED)
    );
    assert_eq!(lines[0].links[0].start_scalar, 3);
    assert_eq!(lines[0].links[0].end_scalar, 5);
    assert_eq!(lines[1].indent_columns, 7);
    assert_eq!(lines[1].spans[0].style.fg, Some(theme::SUBTEXT));
    assert_eq!(lines[2].spans[0].style.fg, Some(theme::MAUVE));
    assert_eq!(inline_anchor_rows(&nodes), vec![("middle".into(), 1)]);
}

#[test]
fn markdown_fence_tags_do_not_disable_lexical_accents() {
    for (fence, expected) in [
        ("python", theme::MAUVE),
        ("unknown", theme::MAUVE),
        ("text", theme::MAUVE),
        ("", theme::MAUVE),
    ] {
        let source = format!("# Example\n\n```{fence}\nif True:\n    print(12)\n```\n");
        let parsed = mant_loader::load_markdown_text(&source, None).unwrap();
        let view = DocumentView::new(&parsed);
        let rendered = view.render(100);
        let hits = rendered.search("if True:");
        assert_eq!(hits.len(), 1);
        let row = &rendered.text.lines[hits[0].row];
        let keyword = row
            .spans
            .iter()
            .find(|span| span.content.starts_with("if"))
            .unwrap();
        assert_eq!(keyword.style.fg, Some(expected), "fence {fence}");
    }
}

#[test]
fn wrapped_highlighted_unicode_links_remain_searchable_and_copyable() {
    let label = "e\u{301}多语言👩‍💻";
    let mut content = bundle();
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::Preformatted {
        children: vec![
            text("echo \""),
            Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "target".into(),
                },
                title: None,
                children: vec![text(label)],
            },
            text("\" # note\n"),
        ],
        language: Some("shell".into()),
        layout: LayoutHint::default(),
        source: None,
    }];
    let before = content.clone();
    let view = DocumentView::new(&content);
    for width in [12, 24, 80, 12] {
        let rendered = view.render(width);
        let hits = rendered.search(label);
        assert_eq!(hits.len(), 1, "width {width}");
        let hit = &hits[0];
        assert_eq!(
            rendered.link_target_at(hit.row, hit.start_column),
            Some(&LinkTarget::Section("target".into()))
        );
        let (row, column) = hit
            .additional_fragments
            .last()
            .map_or((hit.row, hit.end_column), |fragment| {
                (fragment.row, fragment.end_column)
            });
        let selected = rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: hit.row,
                column: hit.start_column,
            },
            focus: TextPosition {
                row,
                column: column - 1,
            },
        });
        assert_eq!(
            selected.lines().map(str::trim_start).collect::<String>(),
            label
        );
        assert_eq!(view.render(width).text, rendered.text);
    }
    assert_eq!(content, before);
}

#[test]
fn long_inline_code_blocks_keep_authored_styles_on_neutral_fallback() {
    let value = "return 12;\n".repeat(crate::code::MAX_BLOCK_BYTES / 11 + 1);
    let nodes = vec![Inline::Strong {
        children: vec![text(&value)],
    }];
    let lines = styled_reference_inline_lines(
        &nodes,
        theme::style(theme::StyleRole::Text),
        None,
        &[],
        true,
        &HashMap::default(),
    );
    assert!(
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.fg == Some(theme::STRONG)
                && span.style.add_modifier.contains(Modifier::BOLD))
    );
    assert_eq!(
        lines
            .iter()
            .map(|line| line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>())
            .collect::<Vec<_>>()
            .join("\n"),
        value
    );
}

#[test]
fn ordinary_inline_code_does_not_select_a_block_grammar() {
    let nodes = vec![Inline::Code {
        value: "#!/bin/sh\nif true; then echo ok; fi".into(),
    }];
    let lines = styled_inline_lines(&nodes, theme::style(theme::StyleRole::Text), None);
    assert!(
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.fg == Some(theme::SUBTEXT_BRIGHT)
                && span.style.bg == Some(theme::SURFACE))
    );
}

#[test]
fn native_fixed_width_font_supplies_a_base_without_erasing_code_accents() {
    // Run this exact input with the pristine reference before the assertions.
    // man_term.c::print_man_node() preserves no-fill rows; roff_term.c's
    // roff_term_pre_ft() maps CW/CR to TERMFONT_NONE, not to a syntax color.
    let source = ".TH HIGHLIGHT 1\n.SH EXAMPLES\n.nf\n.ft CW\nconst char *message = \"text\"; /* note */\n.ft R\n.fi\n";
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let before = content.clone();
    let view = DocumentView::new(&content);
    let rendered = view.render(100);
    let hit = &rendered.search("const char *message")[0];
    let row = &rendered.text.lines[hit.row];
    let text = row
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(text.trim(), "const char *message = \"text\"; /* note */");
    for (needle, foreground) in [
        ("const", theme::MAUVE),
        ("char", theme::MAUVE),
        ("text", theme::BLUE),
        ("/* note", theme::SUBTEXT),
    ] {
        let offset = text.find(needle).unwrap();
        let mut end = 0;
        let span = row
            .spans
            .iter()
            .find(|span| {
                end += span.content.len();
                offset < end
            })
            .unwrap();
        assert_eq!(span.style.fg, Some(foreground), "{needle}");
        assert_eq!(span.style.bg, Some(theme::SURFACE));
    }
    assert_eq!(content, before);
}

#[test]
fn native_no_fill_command_example_uses_lexical_accents_without_mutating_language() {
    // Pristine reference run before adding the assertion:
    // man_term.c::print_man_node() keeps NODE_NOFILL input rows and sends
    // their literal text to term_word(); lexical accents belong to the UI.
    let source = ".TH HIGHLIGHT 1\n.SH EXAMPLES\n.nf\ncurl https://example.org/a --output=\"$HOME/file\" # download\n.fi\n";
    let content: ResolvedContent = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let before = content.clone();
    assert!(
        content
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .flat_map(|section| &section.blocks)
            .any(|block| matches!(block, Block::Preformatted { language: None, .. }))
    );
    let view = DocumentView::new(&content);
    let rendered = view.render(100);
    let hit = &rendered.search("curl https://example.org/a")[0];
    let row = &rendered.text.lines[hit.row];
    assert_eq!(
        row.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
            .trim(),
        "curl https://example.org/a --output=\"$HOME/file\" # download"
    );
    for (needle, foreground) in [
        ("--output", theme::GREEN),
        ("$HOME", theme::PINK),
        ("# download", theme::SUBTEXT),
    ] {
        let offset = row
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
            .find(needle)
            .unwrap();
        let mut end = 0;
        let span = row
            .spans
            .iter()
            .find(|span| {
                end += span.content.len();
                offset < end
            })
            .unwrap();
        assert_eq!(span.style.fg, Some(foreground), "{needle}");
    }
    assert_eq!(content, before);
}
