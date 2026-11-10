//! A retained literal space already carries the old owner's completed rows.

use mant_ir::{Block, Inline, Section, geometry, inline_plain_text};
use mant_loader::ResolvedContent;
use mant_protocol::{ContentSelector, ExcerptSelection, QueryBundle, QueryExcerpt};

const HEADER: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n";
const FOOTER: &str = "\n.SS NEXT\nAFTERMARK\n.SH ENDTEST\nFINISH\n";
const WIDTHS: [usize; 4] = [12, 24, 78, 120];

fn load(source: &str) -> ResolvedContent {
    let input = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    assert_eq!(
        mant_ir::validate_document(input.document.as_ref().unwrap()),
        []
    );
    let wire = serde_json::to_string(&QueryBundle::from(&input)).unwrap();
    let read = ResolvedContent::from(serde_json::from_str::<QueryBundle>(&wire).unwrap());
    assert_eq!(read, input, "{source}");
    read
}

fn unstyle(text: &str) -> String {
    text.replace("\x1b[1m", "").replace("\x1b[0m", "")
}

fn ansi(_: mant_render::TextPresentation, text: &str) -> String {
    format!("\x1b[1m{text}\x1b[0m")
}

fn body_rows(text: &str, content: &str, gap: u16, heading: &str, source: &str) {
    let old_expected = format!("{content}\n{}", "\n".repeat(usize::from(gap)));
    // Do not trim the space-only final literal row. Heading and BODY have
    // known visual indentation; their source scalars are asserted in IR.
    let next = text
        .strip_prefix(&old_expected)
        .unwrap_or_else(|| panic!("{source}: missing {old_expected:?} in {text:?}"));
    assert_eq!(
        next.lines().collect::<Vec<_>>(),
        [format!("  {heading}"), "  AFTERMARK".to_owned()],
        "{source}"
    );
}

fn assert_projection(
    source: &str,
    input: &ResolvedContent,
    content: &str,
    gap: u16,
    heading: &str,
    literal: bool,
) {
    let document = input.document.as_ref().unwrap();
    let first = &document.sections[1];
    let (old, remainder) = first.blocks.split_first().unwrap();
    let children = match old {
        Block::Preformatted { children, .. } if literal => children,
        Block::Paragraph { children, .. } if !literal => children,
        _ => panic!("wrong old owner: {source}\n{old:?}"),
    };
    assert_eq!(inline_plain_text(children), content, "{source}");
    if gap == 0 {
        assert_eq!(remainder, [], "{source}");
    } else {
        assert!(
            matches!(remainder, [Block::VerticalSpace { lines, .. }] if *lines == gap),
            "{source}\n{remainder:?}"
        );
    }
    let [next] = first.children.as_slice() else {
        panic!("NEXT must remain a child section: {source}");
    };
    assert_eq!(first.id.as_str(), "first");
    assert_eq!(
        next.id.as_str(),
        if heading.is_empty() {
            "section"
        } else {
            "next"
        }
    );
    assert_eq!(next.heading.plain_text(), heading, "{source}");
    assert_eq!(next.spacing_before_lines, 0, "{source}");
    let [Block::Paragraph { children, .. }] = next.blocks.as_slice() else {
        panic!("AFTERMARK lost its source owner: {source}");
    };
    assert_eq!(inline_plain_text(children), "AFTERMARK");
    let source_line = |prefix| {
        u32::try_from(
            source
                .lines()
                .position(|line| line.starts_with(prefix))
                .unwrap()
                + 1,
        )
        .unwrap()
    };
    assert_eq!(
        geometry::block_source(old).unwrap().line,
        source_line("ALPHA")
    );
    assert_eq!(next.heading.source.unwrap().line, source_line(".SS "));
    assert_eq!(
        geometry::block_source(&next.blocks[0]).unwrap().line,
        source_line("AFTERMARK")
    );
}

fn select(input: &ResolvedContent, section: &Section) -> QueryExcerpt {
    let excerpt =
        mant_query::select_excerpt(input, &[ContentSelector::id(section.id.as_str())]).unwrap();
    let wire = serde_json::to_string(&excerpt).unwrap();
    let excerpt: QueryExcerpt = serde_json::from_str(&wire).unwrap();
    let [
        ExcerptSelection::DocumentSection {
            section: selected, ..
        },
    ] = excerpt.selections.as_slice()
    else {
        panic!("wrong selected source owner: {}", section.id);
    };
    assert_eq!(selected, section);
    excerpt
}

fn assert_reading(source: &str, input: &ResolvedContent, content: &str, gap: u16, heading: &str) {
    for width in WIDTHS {
        let plain = mant_render::render_query_man_at_width(input, width);
        let decorated = mant_render::render_query_text_at_width(input, width, ansi);
        assert_eq!(plain, unstyle(&decorated), "{source}, width {width}");
        let (_, first) = plain.split_once("FIRST\n").unwrap();
        let (text, _) = first.split_once("\nENDTEST\n").unwrap();
        body_rows(text, content, gap, heading, source);
    }
    let first = &input.document.as_ref().unwrap().sections[1];
    for section in [first, &first.children[0]] {
        let excerpt = select(input, section);
        let text = mant_render::render_excerpt_text(&excerpt);
        assert_eq!(
            text,
            unstyle(&mant_render::render_excerpt_text_with(&excerpt, ansi)),
            "{source}"
        );
        if section.id == first.id {
            let (_, first) = text.split_once("\nFIRST\n").unwrap();
            body_rows(first, content, gap, heading, source);
        } else {
            // The selected child has no old literal or its completed gap.
            assert!(
                text.lines()
                    .map(|row| row.trim_start_matches(' '))
                    .collect::<Vec<_>>()
                    .ends_with(&[heading.trim_start_matches(' '), "AFTERMARK"]),
                "{source}"
            );
        }
    }
}

#[test]
fn literal_motion_keeps_reading_space_without_republishing_completed_rows() {
    // These exact sources ran pinned ASCII/UTF-8/HTML/tree/lint. In
    // term.c:678–700 positive HORIZ contributes NBRSP; term_field defers
    // it until a printable graph. ManT's literal policy keeps one reading
    // space on its separate old row, in addition to the actual old empty
    // passes. The heading executes after the shared flush (man_term.c::post_SH).
    for completed in [1, 2] {
        for advance in [1, 2] {
            for continuation in ["", r"\c"] {
                let passes = r"\p\& ".repeat(completed);
                let source =
                    format!("{HEADER}.nf\nALPHA\\p {passes}\\h'{advance}n'{continuation}{FOOTER}");
                let input = load(&source);
                let old = format!("ALPHA{} ", "\n".repeat(completed + 1));
                let heading = if continuation.is_empty() {
                    " NEXT"
                } else {
                    "NEXT"
                };
                assert_projection(&source, &input, &old, 0, heading, true);
                assert_reading(&source, &input, &old, 0, heading);
            }
        }
    }
}

#[test]
fn authored_blanks_and_real_tail_receipts_are_not_suppressed() {
    // Exact negative controls also ran pristine first. NBRZW can admit
    // a real old empty row; normal filled HORIZ doesn't leave a literal
    // reading cell. Authored NBSP and \~ remain exact owner scalars.
    for (mode, tail, old, gap) in [
        ("", r"\h'1n'", "ALPHA", 1),
        (".nf\n", r"\h'0n'", "ALPHA", 1),
        (".nf\n", " ", "ALPHA", 1),
        (".nf\n", "\u{a0}", "ALPHA\n\n\u{a0}", 0),
        (".nf\n", r"\~", "ALPHA\n\n\u{a0}", 0),
        (".nf\n", r"\h'1n' BETA", "ALPHA\n\n  BETA", 0),
        (".nf\n", r"\zX\h'1n'", "ALPHA\n\nX", 0),
        (".nf\n", r"\fI\h'1n'\fP", "ALPHA\n\n ", 0),
    ] {
        let source = format!("{HEADER}{mode}ALPHA\\p \\p\\& {tail}{FOOTER}");
        let input = load(&source);
        assert_projection(&source, &input, old, gap, " NEXT", !mode.is_empty());
        if tail.starts_with(r"\fI") {
            let Block::Preformatted { children, .. } =
                &input.document.as_ref().unwrap().sections[1].blocks[0]
            else {
                unreachable!();
            };
            assert!(children.iter().any(|child| matches!(
                child,
                Inline::Emphasis { children } if inline_plain_text(children) == "\n\n "
            )));
        }
    }
}

#[test]
fn a_late_reading_space_is_not_attached_to_an_invisible_heading() {
    // Pinned term.c::term_field has no printed HEAD graph here. Its
    // final accepted row still spans the retained old space, so it is
    // not another heading gap. The subsection still owns AFTERMARK.
    let source =
        format!("{HEADER}.nf\nALPHA\\p \\p\\& \\h'1n'\n.SS \\&\nAFTERMARK\n.SH ENDTEST\nFINISH\n");
    let input = load(&source);
    assert_projection(&source, &input, "ALPHA\n\n ", 0, "", true);
    let text = mant_render::render_query_man(&input);
    let (_, first) = text.split_once("\nFIRST\n").unwrap();
    assert_eq!(
        first.split_once("\nENDTEST\n").unwrap().0,
        "ALPHA\n\n \n  AFTERMARK"
    );
}

#[test]
fn actual_section_flush_keeps_the_independent_paragraph_distance() {
    // Fixed CVS five-profile controls: man_term.c::post_SH closes a
    // preceding SH body, and pre_SH/pre_SS execute a separate PD 1 gap.
    // Neither makes the retained motion row another empty literal row.
    for (macro_name, distance) in [("SH", 0), ("SH", 1), ("SS", 1)] {
        let header = HEADER.replace(".PD 0\n", &format!(".PD {distance}\n"));
        let footer = FOOTER.replace(".SS NEXT", &format!(".{macro_name} NEXT"));
        let source = format!("{header}.nf\nALPHA\\p \\p\\& \\h'1n'{footer}");
        let input = load(&source);
        let doc = input.document.as_ref().unwrap();
        let first = &doc.sections[1];
        let next = if macro_name == "SS" {
            let [next] = first.children.as_slice() else {
                panic!("{source}: expected subsection");
            };
            next
        } else {
            assert_eq!(first.children, [], "{source}");
            &doc.sections[2]
        };
        assert_eq!(next.id.as_str(), "next");
        assert_eq!(next.heading.plain_text(), "NEXT");
        assert_eq!(next.spacing_before_lines, distance, "{source}");
        let [Block::Preformatted { children, .. }] = first.blocks.as_slice() else {
            panic!("{source}: extra old owner blocks");
        };
        assert_eq!(inline_plain_text(children), "ALPHA\n\n ");
        let reading = mant_render::render_query_man(&input);
        let (_, rows) = reading.split_once("\nFIRST\n").unwrap();
        let indent = if macro_name == "SS" { "  " } else { "" };
        let expected = format!(
            "ALPHA\n\n \n{}{indent}NEXT\n{indent}AFTERMARK\n",
            "\n".repeat(usize::from(distance))
        );
        assert!(rows.starts_with(&expected), "{source}: {rows:?}");
    }
}

#[test]
fn an_ordinary_literal_flush_also_settles_the_retained_motion_row_once() {
    // Both sources ran pinned CVS first. man_term.c::pre_RS and the
    // final BODY post call term_newln, with no new heading at the seam.
    // In the reading layout the retained old space already occupies
    // the final accepted row, including when another literal follows.
    for (rest, old, rendered) in [
        ("", "ALPHA\n\n ", "ALPHA\n\n "),
        (
            ".RS 2n\nAFTERMARK\n.RE\n.SH ENDTEST\nFINISH\n",
            "ALPHA\n\n \nAFTERMARK",
            "ALPHA\n\n \n  AFTERMARK\nENDTEST\nFINISH",
        ),
    ] {
        let source = format!("{HEADER}.nf\nALPHA\\p \\p\\& \\h'1n'\n{rest}");
        let input = load(&source);
        let first = &input.document.as_ref().unwrap().sections[1];
        let [Block::Preformatted { children, .. }] = first.blocks.as_slice() else {
            panic!("{source}: extra old blocks: {:?}", first.blocks);
        };
        assert_eq!(inline_plain_text(children), old, "{source}");
        let plain = mant_render::render_query_man(&input);
        assert_eq!(plain.split_once("\nFIRST\n").unwrap().1, rendered);
    }
}
