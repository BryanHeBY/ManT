#[test]
fn font_scope_pop_preserves_previous_selection_and_spacing() {
    let mut builder = super::InlineBuilder::new();
    builder.font.select(super::Font::Emphasis);
    builder.font.select(super::Font::Strong);
    builder.with_font_scope(super::Font::Code, |builder| {
        builder.with_font_scope(super::Font::Regular, |builder| {
            builder.tighten_next_boundary();
        });
        assert_eq!(builder.font.current, super::Font::Regular);
        assert_eq!(builder.font.previous, super::Font::Regular);
        assert_eq!(builder.font.display_current(), super::Font::Code);
    });
    assert_eq!(builder.font.current, super::Font::Strong);
    assert_eq!(builder.font.previous, super::Font::Regular);
    assert_eq!(builder.font.display_current(), super::Font::Strong);
    assert!(builder.has_tight_boundary());
}

#[test]
fn prefix_scope_distinguishes_invisible_operands_from_explicit_joins() {
    for explicit in [false, true] {
        let mut builder = super::InlineBuilder::new();
        builder.append(super::text_node("-"));
        builder.with_prefix_join(|builder| {
            builder.append(vec![super::Inline::anchor("operand")]);
            builder.append(super::text_node(""));
            if explicit {
                builder.tighten_next_boundary();
            }
        });
        builder.append(super::text_node("next"));
        let output = builder.finish();
        assert_eq!(
            super::plain_text(&output),
            if explicit { "-next" } else { "- next" }
        );
        assert!(output.iter().any(
            |node| matches!(node, super::Inline::Anchor { id, .. } if id.as_str() == "operand")
        ));
    }
    let mut builder = super::InlineBuilder::new();
    builder.append(super::text_node("-"));
    builder.with_prefix_join(|builder| {
        builder.append(super::text_node("-"));
        builder.with_prefix_join(|builder| builder.append(super::text_node("")));
    });
    builder.append(super::text_node("next"));
    assert_eq!(super::plain_text(&builder.finish()), "-- next");
}

#[test]
fn styled_scopes_preserve_pending_spacing_and_continuation() {
    // Equivalent No/Sm/Em source and its Ns control ran pristine CVS
    // first. These are actual term_word calls (term.c:573-589), not
    // presentation-only IR appends that invent no native boundary.
    for tight in [false, true] {
        let mut builder = super::InlineBuilder::new();
        builder.append_text("FIRST");
        if tight {
            builder.tighten_next_boundary();
        }
        builder.append_scope(|builder| builder.set_spacing("off"), |nodes| nodes);
        builder.append_scope(
            |builder| builder.append_text("SECOND"),
            |children| vec![super::Inline::Emphasis { children }],
        );
        builder.append_text("THIRD");
        assert_eq!(
            super::plain_text(&builder.finish()),
            if tight {
                "FIRSTSECONDTHIRD"
            } else {
                "FIRST SECONDTHIRD"
            }
        );
    }
}

use super::{
    FilledBoundary, Font, InlineBuilder, parse_roff_text, parse_roff_text_with_font, plain_text,
};
use mant_ir::Inline;

#[test]
fn inline_builder_tracks_nested_visible_boundaries_incrementally() {
    let mut builder = InlineBuilder::new();
    builder.append(vec![Inline::anchor("start")]);
    builder.append(vec![Inline::Strong {
        children: vec![Inline::Text {
            value: "first".to_owned(),
        }],
    }]);
    builder.append_filled(
        vec![Inline::Emphasis {
            children: vec![Inline::Text {
                value: "second".to_owned(),
            }],
        }],
        FilledBoundary::Word,
    );
    builder.hard_break();
    builder.hard_break();
    builder.append(vec![Inline::Code {
        value: "third".to_owned(),
    }]);

    assert_eq!(plain_text(&builder.finish()), "first second\nthird");
}

#[test]
fn decodes_fonts_hyphens_and_renderer_links() {
    let nodes =
        parse_roff_text("\\X'tty: link https://example.test'\\fB\\-h\\fR\\X'tty: link' FILE");

    assert_eq!(plain_text(&nodes), "-h FILE");
    assert!(matches!(
        nodes[0],
        Inline::Link {
            target: mant_ir::LinkTarget::External { .. },
            ..
        }
    ));
}

#[test]
fn removes_roff_layout_escapes_without_hiding_literal_punctuation() {
    let source = r"[\|optional\|]\&.\|.\|. \||\|";

    assert_eq!(plain_text(&parse_roff_text(source)), "[optional]... |");
}

#[test]
fn projects_zero_advance_glyphs_after_full_escape_decoding() {
    // CVS term.c keeps TERMP_BACKAFTER through font, device and other
    // formatter controls.  The glyph after `\\z` is still a real glyph:
    // only a later printable glyph at the same formatter position hides
    // it.  Test named and numbered glyphs as well as controls with
    // operands so a local skip parser cannot regress this contract.
    for (source, expected) in [
        (r"TOKEN\z\[rs]", r"TOKEN\"),
        (r"TOKEN\z\N'88'", "TOKENX"),
        (r"TOKEN\zX\fP", "TOKENX"),
        (r"TOKEN\zX\&", "TOKENX"),
        (r"TOKEN\zX END", "TOKENXEND"),
        (r"A\z\h'1n'XB END", "AXB END"),
        (r"A\z\F[mono]XB END", "AB END"),
    ] {
        assert_eq!(plain_text(&parse_roff_text(source)), expected, "{source}");
    }

    let nodes = parse_roff_text(r"A\z\fBXB\fP END");
    assert_eq!(plain_text(&nodes), "AB END");
    assert!(matches!(
        nodes.as_slice(),
        [Inline::Text { value: prefix }, Inline::Strong { children }, Inline::Text { value: suffix }]
            if prefix == "A"
                && suffix == " END"
                && matches!(children.as_slice(), [Inline::Text { value }] if value == "B")
    ));
}

#[test]
fn consumes_groff_colour_and_size_state_around_visible_text() {
    let source = r"The \m[blue]\fBGit User\(cqs Manual\fR\m[]\&\s-2\u[1]\d\s+2 has more detail";
    let nodes = parse_roff_text(source);

    assert_eq!(
        plain_text(&nodes),
        "The Git User’s Manual[1] has more detail"
    );
    assert!(
        nodes
            .iter()
            .any(|node| matches!(node, Inline::Strong { .. }))
    );
}

#[test]
fn preserves_pandoc_verbatim_font_styles() {
    let nodes = parse_roff_text(r"\f[V]code\f[R] \f[VB]bold\f[R] \f[VI]italic\f[R]");

    assert_eq!(plain_text(&nodes), "code bold italic");
    assert!(matches!(nodes.first(), Some(Inline::Code { value }) if value == "code"));
    assert!(nodes.iter().any(|node| matches!(
        node,
        Inline::Strong { children }
            if matches!(children.as_slice(), [Inline::Code { value }] if value == "bold")
    )));
    assert!(nodes.iter().any(|node| matches!(
        node,
        Inline::Emphasis { children }
            if matches!(children.as_slice(), [Inline::Code { value }] if value == "italic")
    )));
}

#[test]
fn decoded_font_spellings_are_never_reinterpreted_as_controls() {
    let generated = parse_roff_text(r"\fB\\fBpackage.json\\fR config\fR");
    assert_eq!(plain_text(&generated), r"\fBpackage.json\fR config");
    assert!(matches!(generated.as_slice(), [Inline::Strong { .. }]));

    let emphasis = parse_roff_text(r"\fI\\fIvalue\\fR\fR");
    assert_eq!(plain_text(&emphasis), r"\fIvalue\fR");
    assert!(matches!(emphasis.as_slice(), [Inline::Emphasis { .. }]));

    let code = parse_roff_text(r"\fC\\fCvalue\\fR\fR");
    assert_eq!(plain_text(&code), r"\fCvalue\fR");
    assert!(matches!(code.as_slice(), [Inline::Code { .. }]));

    let literal = parse_roff_text(r"show \\fBbold\\fR markup");
    assert_eq!(plain_text(&literal), r"show \fBbold\fR markup");
}

#[test]
fn promotes_only_evidenced_sphinx_manual_references() {
    let nodes = parse_roff_text(r"See btrfs\-subvolume(8) \%<> and btrfs(5) \%<> for details.");

    assert_eq!(
        plain_text(&nodes),
        "See btrfs-subvolume(8) and btrfs(5) for details."
    );
    let references = nodes
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link {
                target:
                    mant_ir::LinkTarget::Manual {
                        name,
                        manual_section: Some(manual_section),
                    },
                ..
            } => Some((name.as_str(), manual_section.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(references, [("btrfs-subvolume", "8"), ("btrfs", "5")]);
}

#[test]
fn preserves_empty_destinations_without_a_safe_reference() {
    for source in [
        r"literal \%<>",
        r"group(qgroup) \%<>",
        r"function(0) \%<>",
        r"/tmp/tool(1) \%<>",
        r"user@tool(1) \%<>",
        r"tool(1)\%<>",
    ] {
        assert!(
            plain_text(&parse_roff_text(source)).contains("<>"),
            "empty destination disappeared from {source:?}"
        );
    }
}

#[test]
fn preserves_sphinx_shape_in_no_fill_and_code_content() {
    let no_fill = parse_roff_text_with_font(r"btrfs-subvolume(8) \%<>", Font::Regular, false);
    let code = parse_roff_text_with_font(r"btrfs-subvolume(8) \%<>", Font::Code, true);

    assert_eq!(plain_text(&no_fill), "btrfs-subvolume(8) <>");
    assert_eq!(plain_text(&code), "btrfs-subvolume(8) <>");
    assert!(!no_fill.iter().any(|inline| matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Manual { .. },
            ..
        }
    )));
    assert!(!code.iter().any(|inline| matches!(
        inline,
        Inline::Link {
            target: mant_ir::LinkTarget::Manual { .. },
            ..
        }
    )));
}
