//! Executed no-fill words are not the same as arbitrary empty inline output.
//! The oracle is mandoc CVS HEAD `man_term`/`mdoc_term` and `term_word`. Where groff
//! preserves an empty macro parameter or a font-only row that mandoc does
//! not, these tests deliberately select mandoc rather than mix both policies.

use mant_ir::ResolvedContent;
use mant_ir::{
    Block, Inline,
    visit::{self, Visit},
};
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

// These cases assert term_word rows, not the name-derived SY origin. Every
// exact source was rerun with pristine CVS before accepting that BODY origin.
// Remove display padding without folding authored words or empty/hard rows;
// the separate row-presentation matrix checks the exact native columns.
fn without_line_origins(text: &str) -> String {
    text.split('\n')
        .map(|line| line.trim_start_matches(' '))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_literal_display_switched_to_fill_keeps_source_word_and_indent_policy() {
    for (body, expected) in [
        ("ALPHA\n BETA", "ALPHA\n BETA"),
        ("ALPHA\\c\n BETA", "ALPHA BETA"),
        // mdoc_term.c::termp_sm_pre and term.c::term_word keep NONOSPACE
        // through filled source lines. The exact display source reran all
        // five pristine profiles before accepting this joined spelling.
        (".Sm off\nALPHA\nBETA", "ALPHABETA"),
    ] {
        let text = render_query_text(&query("mdoc", &format!(".fi\n{body}\n.nf\nGAMMA")));
        assert!(text.contains(expected), "{body}: {text:?}");
    }
}

fn query(mode: &str, body: &str) -> ResolvedContent {
    let source = match mode {
        "mdoc" => format!(
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bd -literal\n{body}\n.Ed\nAFTER\n"
        ),
        "SY" => format!(".TH PROBE 1\n.SH TEST\n.SY probe\n.nf\n{body}\n.fi\n.YS\nAFTER\n"),
        _ => format!(".TH PROBE 1\n.SH TEST\n.nf\n{body}\n.fi\nAFTER\n"),
    };
    load_roff_bytes(source.as_bytes()).unwrap()
}

#[test]
fn man_empty_font_macro_operands_and_invisible_glyphs_occupy_a_row() {
    for mode in ["man", "SY"] {
        for request in [
            ".B \"\"", ".I \"\"", ".B \\&", ".I \\&", ".BR \\&", ".BI \\&",
        ] {
            let content = query(mode, &format!("ALPHA\n{request}\nBETA"));
            let text = without_line_origins(&render_query_text(&content));
            assert!(text.contains("ALPHA\n\nBETA"), "{mode}: {request}: {text}");
        }
    }
}

#[derive(serde::Deserialize)]
struct SynopsisFieldFixture {
    oracle_sha256: String,
    cases: Vec<SynopsisFieldCase>,
}

#[derive(serde::Deserialize)]
struct SynopsisFieldCase {
    id: String,
    head: String,
    body: String,
    first_body_no_fill: Option<bool>,
    source: String,
    expected_rows: Vec<String>,
    #[serde(default)]
    reading_rows: Option<Vec<String>>,
    #[serde(default)]
    reading_owner_line: Option<u32>,
}

fn assert_synopsis_parts(case: &SynopsisFieldCase) {
    fn synopsis(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
        if node.kind == libmandoc_rs::NodeKind::Block && node.macro_token.as_deref() == Some("SY") {
            Some(node)
        } else {
            node.children.iter().find_map(synopsis)
        }
    }
    let parsed = libmandoc_rs::Parser::default()
        .parse_bytes("synopsis-field.1", case.source.as_bytes())
        .unwrap();
    let block = synopsis(&parsed.document.root).expect("native SY BLOCK");
    let part = |kind| {
        block
            .children
            .iter()
            .find(|node| node.kind == kind)
            .unwrap()
    };
    let head = part(libmandoc_rs::NodeKind::Head);
    let body = part(libmandoc_rs::NodeKind::Body);
    assert_eq!(
        head.children.is_empty(),
        case.head == "empty",
        "{}",
        case.id
    );
    assert_eq!(
        body.children.is_empty(),
        case.body == "empty",
        "{}",
        case.id
    );
    assert_eq!(
        body.children.first().map(|node| node.flags.no_fill),
        case.first_body_no_fill,
        "{}: pre_SY reads the actual first BODY node",
        case.id
    );
}

fn assert_initial_code_row_policy(case: &SynopsisFieldCase, document: &mant_ir::Document) {
    let Some(owner_line) = case.reading_owner_line else {
        assert_eq!(case.reading_rows, None, "{}", case.id);
        return;
    };
    assert!(case.reading_rows.is_some(), "{}", case.id);
    // The four fi/nf drains create a second complete literal owner whose
    // raw executed rows are [empty, BETA]. The documented Code-block Initial
    // Rows policy removes its one initial LF once, after complete assembly.
    // It does not remove the native row expectation or an internal blank.
    let section = document
        .sections
        .iter()
        .find(|section| section.id.as_str() == "test")
        .unwrap();
    let owners: Vec<_> = section
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Preformatted {
                children, source, ..
            } => Some((children, source)),
            _ => None,
        })
        .collect();
    assert_eq!(owners.len(), 2, "{}", case.id);
    let (children, source) = owners[1];
    let source = source.expect("complete second literal owner keeps its source");
    assert_eq!((source.line, source.column), (owner_line, 2), "{}", case.id);
    assert_eq!(
        children,
        &[
            Inline::Text {
                value: String::new()
            },
            Inline::Text {
                value: "BETA".to_owned()
            },
        ],
        "{}",
        case.id
    );
}

#[test]
fn synopsis_field_posts_keep_empty_rows_and_restore_the_body_budget() {
    // All 27 complete sources ran five pristine profiles first.
    // man_term.c:878-881 flushes HEAD before clearing only NOBREAK;
    // pre_SY(BODY):861-864 restores maxrmargin while retaining name origin.
    // The real HEAD/BODY shape below proves both branches of pre_SY(HEAD),
    // including empty BODY and fill-mode drains. No hard rows or word seams
    // are folded when comparing the rendered regions. Four complete second
    // literal owners separately select the established initial-row reading
    // policy; their full native rows remain in the fixture unchanged.
    // Six adjacent sources also preserve HEAD-to-BODY Tab references and
    // retire bare/cached BACKAFTER at the unconditional HEAD flush.
    // Three nonzero RS bases prove pre_SY assigns mt->offset + len, even
    // after in, and the initially no-fill BODY still posts after fi.
    let fixture: SynopsisFieldFixture =
        serde_json::from_str(include_str!("no_fill_synopsis/field_posts.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.cases.len(), 27);
    let mut failures = Vec::new();
    for case in fixture.cases {
        assert_synopsis_parts(&case);
        let content = load_roff_bytes(case.source.as_bytes()).unwrap();
        let document = content.document.as_ref().unwrap();
        assert_initial_code_row_policy(&case, document);
        let encoded = serde_json::to_string(document).unwrap();
        let decoded: mant_ir::Document = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            *document, decoded,
            "{}: JSON retains the same owners",
            case.id
        );
        let text = without_line_origins(&render_query_text(&content));
        if case.expected_rows.is_empty() {
            let section = document
                .sections
                .iter()
                .find(|section| section.id.as_str() == "test")
                .expect("original empty TEST section remains addressable");
            assert_eq!(section.blocks.len(), 0, "{}", case.id);
            assert_eq!(section.children.len(), 0, "{}", case.id);
        }
        // Every source retains a SY block in TEST even when that block
        // projects no body. man_term.c::pre_SH sees the native child and
        // contributes one PD row before NEXT, independently of TEST's IR.
        let next = document
            .sections
            .iter()
            .find(|section| section.id.as_str() == "next")
            .expect("NEXT section remains addressable");
        assert_eq!(next.spacing_before_lines, 1, "{}", case.id);
        let region = text
            .split_once("\nTEST\n")
            .unwrap()
            .1
            .split_once("\nNEXT\n")
            .unwrap()
            .0;
        let rows: Vec<_> = region.split('\n').collect();
        let reading_rows = case.reading_rows.as_ref().unwrap_or(&case.expected_rows);
        let expected: Vec<_> = reading_rows
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(""))
            .collect();
        if rows != expected {
            failures.push(format!(
                "{}\nnative body {:?}\nreading with NEXT spacing {expected:?}\nproduct {rows:?}",
                case.id, case.expected_rows,
            ));
        }
    }
    assert_eq!(failures.len(), 0, "{}", failures.join("\n"));
}

#[test]
fn empty_mdoc_or_alternating_font_parameters_are_not_literal_blank_rows() {
    // CVS mdoc_term.c::print_mdoc_node() treats an empty TEXT as a blank row
    // only under NODE_LINE; macro operands do not start a source row.
    for (mode, requests) in [
        (
            "mdoc",
            &[".No \"\"", ".Em \"\"", ".Sy \"\"", ".Ar \"\"", ".Cm \"\""][..],
        ),
        ("man", &[".BI \"\"", ".BR \"\"", ".IR \"\""][..]),
    ] {
        for request in requests {
            let text = render_query_text(&query(mode, &format!("ALPHA\n{request}\nBETA")));
            assert!(text.contains("ALPHA\nBETA"), "{mode}: {request}: {text}");
        }
    }
    for request in [".No \\&", ".Em \\&", ".Sy \\&", ".Ar \\&", ".Cm \\&"] {
        let text = render_query_text(&query("mdoc", &format!("ALPHA\n{request}\nBETA")));
        assert!(text.contains("ALPHA\n\nBETA"), "{request}: {text}");
    }
}

#[test]
fn font_only_words_end_the_old_row_without_occupying_the_next_one() {
    for (mode, request) in [
        ("man", ".B \\fB"),
        ("man", "\\fB"),
        ("SY", ".B \\fB"),
        ("mdoc", ".No \\fB"),
        ("mdoc", "\\fB"),
    ] {
        for continued in [false, true] {
            let join = if continued { "\\c" } else { "" };
            let text = without_line_origins(&render_query_text(&query(
                mode,
                &format!("ALPHA{join}\n{request}\nBETA"),
            )));
            assert!(text.contains("ALPHA\nBETA"), "{mode}: {request}: {text}");
        }
        let text = without_line_origins(&render_query_text(&query(
            mode,
            &format!("ALPHA\n{request}"),
        )));
        assert!(text.contains("ALPHA\nAFTER"), "{mode}: {request}: {text}");
    }
}

#[test]
fn state_only_requests_and_continuation_words_do_not_invent_a_row() {
    for mode in ["man", "SY", "mdoc"] {
        for request in [".ft B", "\\c"] {
            let text = render_query_text(&query(mode, &format!("ALPHA\\c\n{request}\nBETA")));
            assert!(text.contains("ALPHABETA"), "{mode}: {request}: {text}");
        }
    }
}

#[test]
fn synopsis_body_executes_fill_mode_changes_instead_of_flattening_every_child() {
    // Rechecked exact source on fixed CVS five profiles. pre_SY/post_SY
    // preserve source fi/nf switches; roff_term_pre_sp's two completed
    // empty rows belong inside the second complete no-fill owner.
    let content = query("SY", "ALPHA\n.fi\nBETA\nSECOND\n.nf\nGAMMA\n.sp 2\nDELTA");
    let text = without_line_origins(&render_query_text(&content));
    assert!(
        text.contains("ALPHA\nBETA SECOND\nGAMMA\n\n\nDELTA"),
        "{text}"
    );
    let blocks = &content.document.as_ref().unwrap().sections[0].blocks;
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, Block::Paragraph { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|block| matches!(block, Block::Preformatted { children, .. } if mant_ir::inline_plain_text(children) == "GAMMA\n\n\nDELTA"))
    );
    assert_eq!(
        mant_ir::validate_document(content.document.as_ref().unwrap()).len(),
        0
    );
}

#[test]
fn literal_display_executes_explicit_fill_switches_and_restores_literal_rows() {
    // Both complete sources rechecked on fixed CVS five profiles. Bd's
    // fill switches retain a filled Paragraph between literal owners;
    // the second owner's executed .sp remains content, including through Bf.
    for wrapped in [false, true] {
        let body = "ALPHA\n.fi\nBETA\nSECOND\n.br\nTHIRD\n.nf\nGAMMA\n.sp 2\nDELTA";
        let body = if wrapped {
            format!(".Bf -emphasis\n{body}\n.Ef")
        } else {
            body.into()
        };
        let content = query("mdoc", &body);
        let text = render_query_text(&content);
        assert!(
            text.contains("ALPHA\nBETA SECOND\nTHIRD\nGAMMA\n\n\nDELTA\nAFTER"),
            "{text}"
        );
        let blocks = &content.document.as_ref().unwrap().sections[0].blocks;
        assert!(matches!(
            blocks.as_slice(),
            [
                Block::Preformatted { .. },
                Block::Paragraph { .. },
                Block::Preformatted { children, .. },
                Block::Paragraph { .. }
            ] if mant_ir::inline_plain_text(children) == "GAMMA\n\n\nDELTA"
        ));
        assert_eq!(
            mant_ir::validate_document(content.document.as_ref().unwrap()).len(),
            0
        );
    }
}

#[test]
fn synopsis_font_changes_survive_spacing_until_the_synopsis_scope_ends() {
    struct Styles {
        strong: bool,
        words: Vec<(String, bool)>,
    }
    impl<'ir> Visit<'ir> for Styles {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            let saved = self.strong;
            match inline {
                Inline::Strong { .. } => self.strong = true,
                Inline::Text { value } => self.words.push((value.clone(), self.strong)),
                _ => {}
            }
            visit::walk_inline(self, inline);
            self.strong = saved;
        }
    }
    let content = query("SY", "ALPHA\n.ft B\n.sp 2\nBETA\nGAMMA");
    let mut styles = Styles {
        strong: false,
        words: Vec::new(),
    };
    styles.visit_document(content.document.as_ref().unwrap());
    for token in ["BETA", "GAMMA"] {
        assert!(
            styles
                .words
                .iter()
                .any(|(word, strong)| word == token && *strong),
            "{token}: {:?}",
            styles.words
        );
    }
    assert!(
        styles
            .words
            .iter()
            .any(|(word, strong)| word == "AFTER" && !strong),
        "{:?}",
        styles.words
    );
    let text = without_line_origins(&render_query_text(&content));
    assert!(text.contains("ALPHA\n\n\nBETA\nGAMMA\nAFTER"), "{text}");
}
