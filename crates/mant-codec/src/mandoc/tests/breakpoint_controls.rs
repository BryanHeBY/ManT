//! Optional breakpoints keep native cells separate from catalog Unicode.

use super::{Block, Inline, Visit, inline_text, parse_manual_bytes, visit};

const MAN_HEADER: &str = ".TH TEST 1 \"2026-10-13\"\n.SH DESCRIPTION\n";
const MDOC_HEADER: &str =
    ".Dd October 13, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn parse(header: &str, body: &str) -> mant_ir::Document {
    let source = format!("{header}{body}");
    let document = parse_manual_bytes(
        std::path::Path::new("native-breakpoint.1"),
        source.as_bytes(),
    )
    .expect("lower exact native source");
    let json = serde_json::to_string(&document).expect("serialize actual JSON text");
    let decoded = serde_json::from_str(&json).expect("decode actual JSON text");
    assert_eq!(document, decoded);
    decoded
}

fn description(document: &mant_ir::Document) -> &[Block] {
    &document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .expect("description section")
        .blocks
}

fn text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                Some(inline_text(children))
            }
            _ => None,
        })
        .collect()
}

fn links(document: &mant_ir::Document) -> Vec<(String, String)> {
    struct Links(Vec<(String, String)>);
    impl<'ir> Visit<'ir> for Links {
        fn visit_inline(&mut self, node: &'ir Inline) {
            if let Inline::Link {
                target: mant_ir::LinkTarget::External { uri },
                children,
                ..
            } = node
            {
                self.0.push((uri.clone(), inline_text(children)));
            }
            visit::walk_inline(self, node);
        }
    }
    let mut links = Links(Vec::new());
    links.visit_document(document);
    links.0
}

#[test]
fn breakpoint_and_word_end_break_consume_the_same_ordered_native_buffer() {
    // All exact complete sources first ran on the pinned pristine five
    // profiles. term_fill():287-305 lets ASCII_BREAK stop a pending \p
    // without becoming a graph. ASCII_NBRZW does arm graph instead.
    for (word, expected) in [
        (r"X\:Y", "XY"),
        (r"X\[:]Y", "XY"),
        (r"X\C':'Y", "XY"),
        (r"X\p\:Y", "X\nY"),
        (r"\p\:Y", ""),
        (r"\zX\:Y", "Y"),
        (r"\z\:XY", "Y"),
        (r"X\zY\p\:Z", "XY\nZ"),
        (r"X\p\zY\:Z", "XY\nZ"),
        (r"\p\zY\:Z", "Y\nZ"),
        (r"\zY\p\:Z", "Y\nZ"),
        (r"\zY\p\: \fBZ", "Y\nZ"),
        (r"X\zY\p\:Z\p W", "XY\nZ\nW"),
        (r"X\zY\p\&Z", "XZ"),
        (r"X\zY\p\:\fBZ", "XY\nZ"),
        (r"X\zY\p Z", "XYZ"),
        (r"X\zY\p\fI Z", "XYZ"),
        (r"X\zY\p\fI\:\fRZ", "XY\nZ"),
        (r"X\zY\:Z", "XZ"),
        (r"X\zY\p\:\zZQ", "XY\nQ"),
        (r"X\p\&Y", "XY"),
    ] {
        let document = parse(MAN_HEADER, &format!("{word}\n"));
        assert_eq!(
            text(description(&document)),
            expected,
            "{word}: {document:#?}"
        );
        assert!(!text(description(&document)).contains('\u{200b}'));
    }
}

#[test]
fn projected_glyph_ownership_keeps_authored_blanks_at_the_next_word_boundary() {
    // Exact sources ran first. term_word() buffers the next word's automatic
    // separator before encode1()'s BACKBEFORE retreat. Older authored blanks
    // behind an already-projected glyph survive; the old glyph cannot be
    // rebound to a new styled/link owner (term.c:573-589 and encode1()).
    for (initial, next, expected) in [
        (r"X\zY\p\: ", ".No Z", "XY\n Z"),
        (r"X\zY\p\: \fB", ".No Z", "XY\n Z"),
        (
            r"X\zY\p\: ",
            ".Lk https://example.test Z",
            "XY\n Z: https://example.test",
        ),
        (r"X\zY\p\: ", r".No \zZQ", "XY\n Q"),
    ] {
        let document = parse(MDOC_HEADER, &format!(".No \"{initial}\"\n{next}\n"));
        assert_eq!(
            text(description(&document)),
            expected,
            "{initial}: {document:#?}"
        );
        if next.starts_with(".Lk ") {
            // The exact native HTML/tree gives Z to this new Lk owner;
            // prior pass Y stays outside it, with one destination only.
            assert_eq!(
                links(&document),
                [("https://example.test".into(), "Z".into())],
                "{document:#?}"
            );
        }
    }
    let document = parse(MAN_HEADER, ".nf\nX\\zY\\p\\:\\c\nZ\n.fi\n");
    assert_eq!(text(description(&document)), "XY\nZ", "{document:#?}");
}

#[test]
fn projected_recovery_and_held_cells_keep_their_actual_source_order() {
    // Exact complete sources ran on the pristine five profiles first.
    // term_word()SPECIAL writes the unknown name as NBRZW, preserving its
    // graph/retreat footprint. ManT's existing reading contract additionally
    // preserves the consumed unknown spelling. The recovery and actual
    // author blank must keep that source order, not be grouped by cell type.
    for (initial, middle, expected) in [
        (r"X\zY\p\:\[unknown] ", "", "XY\n\\[unknown] Z"),
        (r"X\zY\p\: \[unknown]", "", "XY\n \\[unknown]Z"),
        (r"X\zY\p\: ", ".No \\fB\n", "XY\n  Z"),
    ] {
        let document = parse(MDOC_HEADER, &format!(".No \"{initial}\"\n{middle}.No Z\n"));
        assert_eq!(
            text(description(&document)),
            expected,
            "{initial}: {document:#?}"
        );
    }
}

#[test]
fn no_fill_breakpoint_rows_are_cells_without_catalog_glyphs() {
    // term_newln() flushes even a graphless buffered ASCII_BREAK and ends
    // that physical row; it also retires bare BACKAFTER. Explicit Unicode
    // U+200B uses ESCAPE_UNICODE -> encode1(), a separate authored graph.
    // roff_escape.c SPECIAL postvalidation rejects named one-byte ':'
    // forms, so \[:] and \C':' do not occupy that row. Exact runs include
    // their diagnostics; these invalid forms are not claimed lint-clean.
    for header in [MAN_HEADER, MDOC_HEADER] {
        for (middle, expected) in [
            (r"\:", "ALPHA\n\nBETA"),
            (r"\&", "ALPHA\n\nBETA"),
            (r"\[:]", "ALPHA\nBETA"),
            (r"\C':'", "ALPHA\nBETA"),
            (r"\[u200B]", "ALPHA\n\u{200b}\nBETA"),
            (r"\:\c", "ALPHA\nBETA"),
            (r"\p\:", "ALPHA\n\nBETA"),
            (r"\z\:", "ALPHA\n\nBETA"),
        ] {
            let next = if header == MAN_HEADER {
                ".SH NEXT\nEND\n"
            } else {
                ".Sh NEXT\n.No END\n"
            };
            let document = parse(header, &format!(".nf\nALPHA\n{middle}\nBETA\n.fi\n{next}"));
            assert_eq!(
                text(description(&document)),
                expected,
                "{header}{middle}: {document:#?}"
            );
        }
    }
}

#[test]
fn field_rejection_distinguishes_breakpoints_from_zero_width_graphs() {
    // The exact HANG sources ran first. The second \p reaches ASCII_BREAK
    // graphless and rejects Y; \& arms graph and accepts the next pass.
    // The following BODY is a new field and survives both decisions.
    for (cell, expected) in [(r"\:", "X"), (r"\&", "X\n\nY")] {
        let document = parse(
            MDOC_HEADER,
            &format!(
                ".Bl -hang -width 4n\n.It Xo\n.No X\\p\n.No \\p{cell}\n.No Y\n.Xc\n.No BODY\n.El\n"
            ),
        );
        let [Block::DefinitionList { items, .. }] = description(&document) else {
            panic!("expected native definition: {document:#?}");
        };
        assert_eq!(
            inline_text(&items[0].terms[0]),
            expected,
            "{cell}: {items:#?}"
        );
        assert_eq!(text(&items[0].description), "BODY", "{cell}: {items:#?}");
    }
}

#[test]
fn html_link_identity_keeps_the_catalog_scalar_without_changing_terminal_text() {
    // html.c::print_encode(norecurse=1) uses chars.c's U+200B and consumes
    // HTML_SKIPCHAR for that special. The terminal's ASCII_BREAK displays
    // no scalar and never consumes BACKAFTER. Both exact UR fixtures ran
    // before assertions; targets were read from the pristine HTML href.
    for (target, expected_target, expected_text) in [
        (
            r"https://example.test/a\:b",
            "https://example.test/a\u{200b}b",
            "label <https://example.test/ab> after",
        ),
        (
            r"https://example.test/a\z\:b",
            "https://example.test/ab",
            "label <https://example.test/a> after",
        ),
    ] {
        let document = parse(MAN_HEADER, &format!(".UR {target}\nlabel\n.UE\nafter\n"));
        assert_eq!(
            links(&document),
            [(expected_target.into(), "label".into())],
            "{document:#?}"
        );
        assert_eq!(text(description(&document)), expected_text, "{document:#?}");
    }
}
