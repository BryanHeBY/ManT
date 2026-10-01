use super::*;

#[test]
fn authored_an_words_use_no_fill_source_rows() {
    // Exact forms checked with fixed CVS -Tascii/-Tlint. In
    // mdoc_term.c::print_mdoc_node(), NODE_NOFILL/NODE_LINE executes before
    // termp_an_pre(); mode-only An changes SPLIT without emitting a word.
    for mode in ["", ".An -split\n", ".An -nosplit\n"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n{mode}.An Alice\n.An Bob\n.fi\n"
        );
        let document =
            parse_manual_bytes(std::path::Path::new("no-fill-authors.1"), source.as_bytes())
                .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "Alice\nBob"),
            "{mode}: {document:#?}"
        );
    }
}

#[test]
fn author_mode_request_does_not_render_rejected_operands() {
    // Exact input checked with fixed CVS -Tascii/-Tlint. The parser reports
    // excess An operands; mdoc_term.c::termp_an_pre() returns 0 for both
    // author modes, so neither operand is visited as a formatter word.
    for mode in ["split", "nosplit"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n.An -{mode} Alice\n.An Bob\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("author-mode-extra-operand.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "Bob"),
            "{mode}: {document:#?}"
        );
    }
}

#[test]
fn author_mode_request_respects_no_fill_source_continuation() {
    // Exact split/nosplit forms checked with fixed CVS -Tascii/-Tlint.
    // mdoc_term.c::print_mdoc_node() skips NODE_LINE's term_newln() under
    // TERMP_NONEWLINE; termp_an_pre() changes only the author mode.
    for mode in ["split", "nosplit"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\nALPHA\\c\n.An -{mode}\nBETA\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("author-mode-no-fill-continuation.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == "ALPHABETA"),
            "{mode}: {document:#?}"
        );
    }
}

#[test]
fn bare_zero_advance_keeps_generated_closing_glyph_in_its_row() {
    // Exact enclosure and function forms rechecked with pristine ASCII/
    // UTF-8/tree/lint. Ao's UTF-8 device glyphs are ⟨⟩, while its ASCII
    // fallback is <>. term.c::term_word() buffers a glyph after bare \z;
    // mdoc_term.c's post emits it before the next NODE_LINE term_newln().
    for (scope, expected) in [
        (".Ao\n\\z\n.Ac", "⟨\n⟩\nNEXT"),
        (".Bo\n\\z\n.Bc", "[\n]\nNEXT"),
        (".Fo call\n\\z\n.Fc", "call(\n)\nNEXT"),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n{scope}\nNEXT\n.fi\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("bare-zero-generated-post.1"),
            source.as_bytes(),
        )
        .unwrap();
        assert!(
            matches!(document.sections[1].blocks.as_slice(), [Block::Preformatted { children, .. }] if inline_text(children) == expected),
            "{scope}: {document:#?}"
        );
    }
}

#[test]
fn author_split_restarts_an_overrun_tag_field() {
    // Fixed CVS -Tascii: an author split that overruns the tag field ends
    // its row (term.c:250-253 through mdoc_term.c:1084-1085), but the
    // field restarts — NOBREAK and BRIND survive until the item post — so
    // the next author word is the restarted field's first word and BODY
    // shares ITS row (`LONGTEXT A` / `Bob     BODY`), not a fresh block.
    let item = definition_item_from_source(
        ".Bl -tag -width 6n\n.It Xo\n.No LONGTEXT\n.No A\n.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n",
    );
    assert_eq!(
        item.layout.head_body_relation,
        HeadBodyRelation::RunIn,
        "{item:#?}"
    );
    let rows = item
        .terms
        .last()
        .map(|term| {
            term.split(|node| matches!(node, Inline::LineBreak { .. }))
                .count()
        })
        .unwrap_or_default();
    assert_eq!(rows, 2, "{item:#?}");
}

#[test]
fn author_split_families_keep_the_reference_row_shapes() {
    // Fixed CVS -Tascii, one shape per (style, head) pair; the split
    // marker's term_newln never clears HANG, so a hang head stays on one
    // row through the split, while a tag head ends its row only when the
    // field overran (term.c:250-253 through mdoc_term.c:1084-1085):
    // - tag/LONGTEXT: `LONGTEXT` / `Bob     BODY` — overrun restart, BODY
    //   shares the restarted row;
    // - tag/A: `A  Bob  BODY` — the fitting field keeps its row;
    // - hang/LONGTEXT and hang/A: one row with BODY behind the trailspace.
    for (style, head, expected_rows, expected_relation) in [
        ("tag", "LONGTEXT", 2, HeadBodyRelation::RunIn),
        ("tag", "A", 1, HeadBodyRelation::RunIn),
        ("hang", "LONGTEXT", 1, HeadBodyRelation::RunIn),
        ("hang", "A", 1, HeadBodyRelation::RunIn),
    ] {
        let item = definition_item_from_source(&format!(
            ".Bl -{style} -width 6n\n.It Xo\n.No {head}\n.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        ));
        assert_eq!(
            item.layout.head_body_relation, expected_relation,
            "{style}/{head}: {item:#?}"
        );
        let rows = item
            .terms
            .last()
            .map(|term| {
                term.split(|node| matches!(node, Inline::LineBreak { .. }))
                    .count()
            })
            .unwrap_or_default();
        assert_eq!(rows, expected_rows, "{style}/{head}: {item:#?}");
    }
}

#[test]
fn bare_zero_advance_crosses_only_structures_without_generated_words() {
    for (label, body, expected) in [
        ("plain-list", ".Bl -item -compact\n.It\n.No B C\n.El", "BC"),
        (
            "literal-display",
            ".Bd -literal -compact\n.No B C\n.Ed",
            "BC",
        ),
        ("one-line-display", ".D1 B C", "BC"),
        ("literal-one-line-display", ".Dl B C", "BC"),
        ("bibliography", ".Rs\n.%A B C\n.Re", "BC."),
        (
            "bullet-generated-word",
            ".Bl -bullet -compact\n.It\n.No B C\n.El",
            "B C",
        ),
        (
            "ordered-generated-word",
            ".Bl -enum -compact\n.It\n.No B C\n.El",
            "B C",
        ),
    ] {
        let source =
            format!(".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\z\n{body}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("zero-advance-structure-{label}.1")),
            source.as_bytes(),
        )
        .expect("parse zero-advance structural boundary");
        let visible = projected_document_text(&document);
        assert!(
            visible.contains(expected),
            "{label}: expected {expected:?} in {visible:?}; {:#?}",
            document.sections
        );
        if expected == "BC" {
            assert!(!visible.contains("B C"), "{label}: {visible:?}");
        }
    }

    // tbl_term.c explicitly clears both backtracking flags before each cell;
    // tables are not transparent structural owners for a bare `\z`.
    let table = parse_manual_bytes(
        std::path::Path::new("zero-advance-structure-table.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\z\n.TS\nl.\nB C\n.TE\n",
    )
    .expect("parse zero-advance table boundary");
    assert!(visible_document_text(&table).contains("B C"));

    let completed_and_armed = parse_manual_bytes(
        std::path::Path::new("zero-advance-structure-active-cell.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\zX\\z\n.Bd -literal -compact\n.No B C\n.Ed\n",
    )
    .expect("parse active zero-advance cell before a display");
    let visible = projected_document_text(&completed_and_armed);
    assert!(visible.contains("XB C"), "{visible:?}");
    assert!(!visible.contains("XBC"), "{visible:?}");

    let man = parse_manual_bytes(
        std::path::Path::new("zero-advance-man-relative-scope.1"),
        b".TH PROBE 1\n.SH DESCRIPTION\n\\z\n.RS 4\nB C\n.RE\n",
    )
    .expect("parse man zero-advance structural boundary");
    let visible = projected_document_text(&man);
    assert!(visible.contains("BC"), "{visible:?}");
    assert!(!visible.contains("B C"), "{visible:?}");
}
