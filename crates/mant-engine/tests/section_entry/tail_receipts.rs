//! A completed blank belongs to the old tail only until that owner prints again.

use mant_ir::{Block, Inline, geometry, inline_plain_text};
use mant_loader::ResolvedContent;
use mant_protocol::{ContentSelector, ExcerptSelection, QueryBundle, QueryExcerpt};

const HEADER: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n";
const FOOTER: &str = ".SH ENDTEST\nFINISH\n";
const PREFIX: &str = r"ALPHA\p \p\&";
const BODY: &str = "AFTERMARK";

#[derive(Clone, Copy)]
struct Tail<'a> {
    inline: &'a str,
    blank_rows: u16,
}

fn load(source: &str) -> ResolvedContent {
    let input = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    assert_eq!(
        mant_ir::validate_document(input.document.as_ref().unwrap()),
        [],
    );
    let wire = serde_json::to_string(&QueryBundle::from(&input)).unwrap();
    let read = ResolvedContent::from(serde_json::from_str::<QueryBundle>(&wire).unwrap());
    assert_eq!(read, input, "{source}");
    read
}

fn reading_rows(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(|row| row.trim_start_matches(' '))
}

fn interval(text: &str) -> Vec<&str> {
    reading_rows(text)
        .skip_while(|row| *row != "FIRST")
        .skip(1)
        .take_while(|row| *row != "ENDTEST")
        .collect()
}

fn assert_tail(source: &str, old: Tail<'_>, title: &str, id: &str) -> ResolvedContent {
    let input = load(source);
    let document = input.document.as_ref().unwrap();
    let first = &document.sections[1];
    assert_eq!(first.id.as_str(), "first");
    let (content, tail) = first.blocks.split_first().unwrap();
    let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = content else {
        panic!("{source}: {content:?}");
    };
    // A real retained .br may close the inline row. The full reading
    // assertion below checks whether that ending produces a physical blank.
    assert_eq!(
        inline_plain_text(children).trim_end_matches('\n'),
        old.inline,
        "{source}",
    );
    let actual_tail: u16 = tail
        .iter()
        .map(|block| match block {
            Block::VerticalSpace { lines, .. } => *lines,
            _ => panic!("unexpected old owner suffix: {source}\n{block:?}"),
        })
        .sum();
    assert_eq!(actual_tail, old.blank_rows, "{source}");

    let [next] = first.children.as_slice() else {
        panic!("expected one authored subsection: {source}");
    };
    assert_eq!(next.id.as_str(), id, "{source}");
    assert_eq!(
        next.heading.plain_text().trim_start_matches(' '),
        title,
        "{source}",
    );
    assert_eq!(next.spacing_before_lines, 0, "{source}");
    let [Block::Paragraph { children, .. }] = next.blocks.as_slice() else {
        panic!("expected a filled successor BODY: {source}");
    };
    assert_eq!(inline_plain_text(children), BODY, "{source}");
    let heading_line = source.lines().position(|s| s.starts_with(".SS ")).unwrap() + 1;
    let old_line = geometry::block_source(content).unwrap().line as usize;
    assert!(old_line < heading_line, "{source}");
    assert!(
        source
            .lines()
            .nth(old_line - 1)
            .is_some_and(|row| row.contains("ALPHA")
                || row.starts_with(".UR ")
                || row.starts_with(".MT ")),
        "{source}"
    );
    assert_eq!(
        next.heading.source.unwrap().line as usize,
        heading_line,
        "{source}"
    );
    assert_eq!(
        geometry::block_source(&next.blocks[0]).unwrap().line as usize,
        heading_line + 1,
        "{source}",
    );

    let heading_and_body: Vec<_> = title.split_terminator('\n').chain([BODY]).collect();
    let rows: Vec<_> = old
        .inline
        .split('\n')
        .chain(std::iter::repeat_n("", usize::from(old.blank_rows)))
        .chain(heading_and_body.iter().copied())
        .collect();
    assert_eq!(
        interval(&mant_render::render_query_man(&input)),
        rows,
        "{source}"
    );
    // Selection of the old section includes the child and must preserve
    // the same exact rows. Selection of the child must not acquire old rows.
    for owner in [first, next] {
        let excerpt =
            mant_query::select_excerpt(&input, &[ContentSelector::id(owner.id.as_str())]).unwrap();
        let wire = serde_json::to_string(&excerpt).unwrap();
        let excerpt: QueryExcerpt = serde_json::from_str(&wire).unwrap();
        let [ExcerptSelection::DocumentSection { section, .. }] = excerpt.selections.as_slice()
        else {
            panic!("selection lost {}: {source}", owner.id);
        };
        assert_eq!(section, owner, "{source}");
        let selected = mant_render::render_excerpt_text(&excerpt);
        let actual: Vec<_> = reading_rows(&selected).collect();
        let expected: Vec<_> = if owner.id == first.id {
            std::iter::once("FIRST")
                .chain(rows.iter().copied())
                .collect()
        } else {
            heading_and_body.clone()
        };
        assert!(
            actual.ends_with(&expected),
            "{source}\n{actual:?} != {expected:?}"
        );
    }
    input
}

#[test]
fn old_empty_passes_stay_in_order_through_section_handoff() {
    // These complete sources ran pristine ASCII/UTF-8/HTML/tree/lint.
    // man_term.c::pre_SS with PD 0 leaves the BODY word live. In term.c,
    // NBRZW can admit a blank pass; term_flushln outputs each accepted
    // pass in order. The old tail changes only on an accepted old glyph.
    // When the device prints BETA NEXT or XNEXT together, structured reading
    // splits at the authored owner, without adding another blank.
    for mode in ["", ".nf\n"] {
        for (old_suffix, head, title, id, inline, blanks) in [
            (" BETA", "NEXT", "NEXT", "next", "ALPHA\n\nBETA", 0),
            (r" \p\& BETA", "NEXT", "NEXT", "next", "ALPHA\n\n\nBETA", 0),
            ("", "NEXT", "NEXT", "next", "ALPHA", 1),
            (r" BETA\p", "NEXT", "NEXT", "next", "ALPHA\n\nBETA", 0),
            (r" \p BETA", "NEXT", "", "next", "ALPHA", 2),
            (r" \zX", "NEXT", "NEXT", "next", "ALPHA\n\nX", 0),
            (r" \zX\p", "NEXT", "NEXT", "next", "ALPHA\n\nX", 0),
            (" BETA", r"\&", "", "section", "ALPHA\n\nBETA", 0),
            (
                " BETA",
                r"NEXT\p WORD",
                "NEXT\nWORD",
                "next-p-word",
                "ALPHA\n\nBETA",
                0,
            ),
            ("", r"\&\p NEXT", "\nNEXT", "p-next", "ALPHA", 1),
        ] {
            let source =
                format!("{HEADER}{mode}{PREFIX}{old_suffix}\n.SS {head}\n{BODY}\n{FOOTER}");
            assert_tail(
                &source,
                Tail {
                    inline,
                    blank_rows: blanks,
                },
                title,
                id,
            );
        }
    }
}

#[test]
fn real_flushes_and_parser_removed_breaks_keep_their_distinct_rows() {
    // CVS tree proves the first .br survives because of the next \& TEXT,
    // while a .br immediately before first SS is removed (lint warns).
    // roff_term.c::roff_term_pre_br ends the row; pre_sp adds a real gap.
    for mode in ["", ".nf\n"] {
        for (controls, blank_rows) in [(".br\n\\&\n", 0), (".br\n", 0), (".sp 1\n", 1)] {
            let source =
                format!("{HEADER}{mode}{PREFIX} BETA\n{controls}.SS NEXT\n{BODY}\n{FOOTER}");
            assert_tail(
                &source,
                Tail {
                    inline: "ALPHA\n\nBETA",
                    blank_rows,
                },
                "NEXT",
                "next",
            );
        }
    }
    for (mode, rows) in [
        ("", &["ALPHA", "", "BETA AFTERMARK"][..]),
        (".nf\n", &["ALPHA", "", "BETA", BODY]),
    ] {
        let source = format!("{HEADER}{mode}{PREFIX} BETA\n{BODY}\n{FOOTER}");
        let input = load(&source);
        assert_eq!(
            interval(&mant_render::render_query_man(&input)),
            rows,
            "{source}"
        );
        assert_eq!(input.document.as_ref().unwrap().sections[1].children, []);
    }
}

#[test]
fn styled_and_linked_old_glyphs_retire_tail_receipts_in_their_source_owner() {
    // In pinned CVS, term.c::encode1 emits the original font into the
    // same pass. man_term.c::post_UR renders the target while the old
    // input is still live, regardless of whether it is UR or MT.
    for mode in ["", ".nf\n"] {
        let source = format!("{HEADER}{mode}\\fI{PREFIX} BETA\\fP\n.SS NEXT\n{BODY}\n{FOOTER}");
        let input = assert_tail(
            &source,
            Tail {
                inline: "ALPHA\n\nBETA",
                blank_rows: 0,
            },
            "NEXT",
            "next",
        );
        let first = &input.document.as_ref().unwrap().sections[1];
        let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) =
            &first.blocks[0]
        else {
            unreachable!();
        };
        assert!(
            children
                .iter()
                .any(|node| matches!(node, Inline::Emphasis { children }
            if inline_plain_text(children).ends_with("BETA"))),
            "{source}"
        );

        for (open, close, target, uri, extra_blank) in [
            ("UR", "UE", "https://audit.test/", "https://audit.test/", ""),
            (
                "MT",
                "ME",
                "audit@example.test",
                "mailto:audit@example.test",
                r" \p\&",
            ),
        ] {
            let source = format!(
                "{HEADER}{mode}.{open} {target}\n{PREFIX}{extra_blank} BETA\n.{close}\n.SS NEXT\n{BODY}\n{FOOTER}"
            );
            let separators = if extra_blank.is_empty() {
                "\n\n"
            } else {
                "\n\n\n"
            };
            let label = format!("ALPHA{separators}BETA");
            let inline = format!("{label} <{target}>");
            let input = assert_tail(
                &source,
                Tail {
                    inline: &inline,
                    blank_rows: 0,
                },
                "NEXT",
                "next",
            );
            let first = &input.document.as_ref().unwrap().sections[1];
            let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) =
                &first.blocks[0]
            else {
                unreachable!();
            };
            assert!(
                matches!(children.first(), Some(Inline::Link { target, children, .. })
                if target.to_uri().as_deref() == Some(uri) && inline_plain_text(children) == label),
                "{source}"
            );
        }
    }
}
