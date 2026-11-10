//! Keep executed rows with their owners through full text, selection and wire transport.

use mant_ir::{Block, Section, geometry, inline_plain_text};
use mant_protocol::{ContentSelector, ExcerptSelection, QueryBundle};

#[path = "pending_rows/controls.rs"]
mod controls;

const HEADER: &str = ".TH CHECK 1 \"2026-11-10\"\n.SH NAME\ncheck \\- audit\n.SH FIRST\n.PD 0\n";
const FOOTER: &str = ".SH ENDTEST\nFINISH\n";
const BODY: &str = "AFTERMARK";

#[derive(Clone, Copy)]
struct Case<'a> {
    input: &'a str,
    id: &'a str,
    title: &'a str,
    old_rows: &'a [&'a str],
    body: &'a str,
    rows: &'a [&'a str],
}

fn interval(rendered: &str) -> Vec<&str> {
    rendered
        .lines()
        .map(|row| row.trim_start_matches(' '))
        .skip_while(|row| *row != "FIRST")
        .skip(1)
        .take_while(|row| *row != "ENDTEST")
        .collect()
}

fn body_text(section: &Section) -> Vec<String> {
    section
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                Some(inline_plain_text(children))
            }
            _ => None,
        })
        .flat_map(|text| {
            text.lines()
                .map(|row| row.trim_start_matches(' ').to_owned())
                .filter(|row| !row.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn assert_case(case: Case<'_>) {
    let source = format!("{HEADER}{}{FOOTER}", case.input);
    let input = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    assert_eq!(
        mant_ir::validate_document(input.document.as_ref().unwrap()),
        []
    );
    let serialized = serde_json::to_string(&QueryBundle::from(&input)).unwrap();
    let roundtrip = mant_loader::ResolvedContent::from(
        serde_json::from_str::<QueryBundle>(&serialized).unwrap(),
    );
    assert_eq!(input, roundtrip, "{source}");
    let document = roundtrip.document.as_ref().unwrap();
    let first = &document.sections[1];
    assert_eq!(first.id.as_str(), "first");
    assert_eq!(body_text(first), case.old_rows, "{source}");
    let [target] = first.children.as_slice() else {
        panic!("one authored target section: {source}");
    };
    assert_eq!(target.id.as_str(), case.id, "{source}");
    // Leading formatter columns are layout; do not strip a physical row.
    assert_eq!(
        target.heading.plain_text().trim_start_matches(' '),
        case.title,
        "{source}"
    );
    assert_eq!(body_text(target).join("\n"), case.body, "{source}");
    assert_eq!(
        target.source.as_ref().unwrap().line as usize,
        source
            .lines()
            .position(|line| line.starts_with(".SS "))
            .unwrap()
            + 1,
        "{source}",
    );
    let paragraph = target.blocks.last().unwrap();
    assert_eq!(
        geometry::block_source(paragraph).unwrap().line as usize,
        source
            .lines()
            .position(|line| line.starts_with(BODY))
            .unwrap()
            + 1,
        "{source}",
    );
    let rendered = mant_render::render_query_man(&roundtrip);
    assert_eq!(interval(&rendered), case.rows, "{source}");
    let excerpt = mant_query::select_excerpt(&roundtrip, &[ContentSelector::id(case.id)]).unwrap();
    let [ExcerptSelection::DocumentSection { section, .. }] = excerpt.selections.as_slice() else {
        panic!("selected the wrong owner: {source}");
    };
    assert_eq!(section, target, "{source}");
    assert!(
        mant_render::render_excerpt_text(&excerpt)
            .trim_end_matches('\n')
            .ends_with(case.body),
        "{source}",
    );
}

#[test]
fn old_pending_rows_do_not_become_leading_heading_rows() {
    // Pinned CVS man_term.c::pre_SS / post_SH, term.c::term_flushln:
    // the marker before HEAD closes ALPHA once, while markers in HEAD
    // and BODY still create their own lines. ASCII and UTF-8 agreed.
    for (input, word) in [
        (".nf\nALPHA\\p\n.SS NEXT\nAFTERMARK\n", "ALPHA"),
        (".nf\nALPHA\\p\\p\\p\n.SS NEXT\nAFTERMARK\n", "ALPHA"),
        (".nf\nALPHA\\p\\&\n.SS NEXT\nAFTERMARK\n", "ALPHA"),
        (".nf\nALPHA\\p\\:\n.SS NEXT\nAFTERMARK\n", "ALPHA"),
        ("ALPHA\\p\n.SS NEXT\nAFTERMARK\n", "ALPHA"),
        (
            ".nf\n\\fBALPHAALPHA\\fR\\p\n.SS NEXT\nAFTERMARK\n",
            "ALPHAALPHA",
        ),
        (".nf\n日本\\p\n.SS NEXT\nAFTERMARK\n", "日本"),
    ] {
        assert_case(Case {
            input,
            id: "next",
            title: "NEXT",
            old_rows: &[word],
            body: BODY,
            rows: &[word, "NEXT", BODY],
        });
    }
    for (input, id, old, rows) in [
        (
            ".nf\nALPHA\\p\n.SS NEXT\\p WORD\nAFTERMARK\n",
            "next-p-word",
            &["ALPHA"][..],
            &["ALPHA", "NEXT", "WORD", BODY][..],
        ),
        (
            ".nf\nALPHA\\p\n.SS NEXT\\p\\p WORD\nAFTERMARK\n",
            "next-p-p-word",
            &["ALPHA"][..],
            &["ALPHA", "NEXT", "WORD", BODY][..],
        ),
        (
            ".nf\n.SS NEXT\\p WORD\nAFTERMARK\n",
            "next-p-word",
            &[][..],
            &["NEXT", "WORD", BODY][..],
        ),
    ] {
        assert_case(Case {
            input,
            id,
            title: "NEXT\nWORD",
            old_rows: old,
            body: BODY,
            rows,
        });
    }
    for (input, old, rows) in [
        (
            ".nf\nALPHA\\p\n.SS NEXT\nAFTERMARK\\p BODYWORD\n",
            &["ALPHA"][..],
            &["ALPHA", "NEXT", BODY, "BODYWORD"][..],
        ),
        (
            ".nf\n.SS NEXT\nAFTERMARK\\p BODYWORD\n",
            &[][..],
            &["NEXT", BODY, "BODYWORD"][..],
        ),
    ] {
        assert_case(Case {
            input,
            id: "next",
            title: "NEXT",
            old_rows: old,
            body: "AFTERMARK\nBODYWORD",
            rows,
        });
    }
}

#[test]
fn rejected_prior_input_keeps_authored_section_without_publishing_its_title() {
    // With PD 0 the heading reaches the rejected pass. CVS tree retains
    // authored NEXT but removes the immediate .br in the third input.
    for input in [
        "\\p BETA\n.SS NEXT\nAFTERMARK\n",
        ".nf\n\\p BETA\n.SS NEXT\nAFTERMARK\n",
        "\\p BETA\n.br\n.SS NEXT\nAFTERMARK\n",
    ] {
        assert_case(Case {
            input,
            id: "next",
            title: "",
            old_rows: &[],
            body: BODY,
            rows: &["", BODY],
        });
    }
}

#[test]
fn section_handoff_keeps_accepted_prefix_and_runs_heading_controls_in_place() {
    // CVS term_fill restarts graph state after each accepted pass. An
    // accepted ALPHA can precede rejection; NBRZW in HEAD instead admits
    // one real blank there. The prior owner's blank is distinct.
    for (input, id, title, rows) in [
        (
            "ALPHA\\p \\p BETA\n.SS NEXT\nAFTERMARK\n",
            "next",
            "",
            &["ALPHA", "", BODY][..],
        ),
        (
            ".nf\nALPHA\\p \\p BETA\n.SS NEXT\nAFTERMARK\n",
            "next",
            "",
            &["ALPHA", "", BODY][..],
        ),
        (
            ".nf\nALPHA\\p\n.SS \\p NEXT\nAFTERMARK\n",
            "p-next",
            "",
            &["ALPHA", "", BODY][..],
        ),
        (
            ".nf\nALPHA\\p\n.SS \\&\\p NEXT\nAFTERMARK\n",
            "p-next",
            "\nNEXT",
            &["ALPHA", "", "NEXT", BODY][..],
        ),
        (
            ".nf\nALPHA\\p \\p\\&\n.SS \\&\\p NEXT\nAFTERMARK\n",
            "p-next",
            "\nNEXT",
            &["ALPHA", "", "", "NEXT", BODY][..],
        ),
    ] {
        assert_case(Case {
            input,
            id,
            title,
            old_rows: &["ALPHA"],
            body: BODY,
            rows,
        });
    }
}
