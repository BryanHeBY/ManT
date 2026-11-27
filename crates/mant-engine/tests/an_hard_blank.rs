//! HEAD row settlement through the public JSON bundle.
#![cfg(feature = "roff")]

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn rows(body: &str) -> Vec<String> {
    let source = format!("{HEADER}{body}");
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    mant_render::render_query_man(&decoded.into())
        .split_once("DESCRIPTION\n")
        .unwrap()
        .1
        .trim_end_matches('\n')
        .lines()
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn head_settlement_claims_only_the_represented_row() {
    // These exact sources were run with pristine CVS -Tascii and -Tutf8.
    // term.c::term_fill accepts separate word-end markers at their spaces;
    // term_flushln and mdoc_term.c::termp_it_post settle the actual rows.
    for (head, body, expected) in [
        (r"A\p \p ", "", vec!["A", "", "C"]),
        (r"A\p \p ", "B\n", vec!["A", "", "C"]),
        (r"A\p\p ", "", vec!["A", "", "C"]),
        (r"A\p \p", "", vec!["A", "", "C"]),
        (r"A\p ", "", vec!["A", "", "C"]),
        (r"A\p\p ", "B\n", vec!["A", "B", "C"]),
        (r"A\p \p", "B\n", vec!["A", "B", "C"]),
        (r"A\p ", "B\n", vec!["A", "B", "C"]),
    ] {
        let source = format!(".Bl -inset\n.It No \"{head}\"\n{body}.br\nC\n.El\n");
        assert_eq!(rows(&source), expected, "{source}");
    }
}

#[test]
fn public_json_keeps_the_hard_row_in_the_item_description() {
    use mant_ir::{Block, HeadBodyRelation, Inline, ResolvedContent};

    // term.c::term_flushln closes A at the loop endline. term_fill rejects
    // the suffix, including B when present, and the tail ends the empty row.
    for body in ["", "B\n"] {
        let source = format!("{HEADER}.Bl -inset\n.It No \"A\\p \\p \"\n{body}.br\nC\n.El\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let content = ResolvedContent::from(decoded);
        let document = content.document.unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{source}");
        };
        let item = &items[0];
        assert_eq!(item.head_body_relation, HeadBodyRelation::Separate);
        let Block::Paragraph { children, .. } = &item.description[0] else {
            panic!("{source}");
        };
        let line = children
            .iter()
            .position(|node| matches!(node, Inline::LineBreak {}))
            .unwrap();
        let body = children
            .iter()
            .position(|node| matches!(node, Inline::Text { value } if value == "C"))
            .unwrap();
        assert!(line < body, "{children:?}");
    }
}

#[test]
fn public_json_places_the_generated_inset_blank_after_the_body_break() {
    use mant_ir::{Block, Inline, ResolvedContent, inline_plain_text};

    // The empty No operand writes an automatic SP (mdoc_term.c:354-378,
    // term.c:573-590). BODY's flush skips that SP and prints the generated
    // NBSP on the next row (term.c:205-220, 389-428).
    let source =
        format!("{HEADER}.Bl -inset\n.It Xo\n.No alpha\\p\n.No \"\"\n.Xc\n.No tail text\n.El\n");
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let document = ResolvedContent::from(decoded).document.unwrap();
    let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
        panic!("{source}");
    };
    let item = &items[0];
    assert!(item.inline_term());
    let [Block::Paragraph { children, .. }] = &item.description[..] else {
        panic!("{item:#?}");
    };
    assert!(matches!(children.first(), Some(Inline::LineBreak {})));
    assert_eq!(inline_plain_text(children), "\n tail text");
}

#[test]
fn rejected_unmodeled_tail_does_not_reopen_its_detached_head_row() {
    // Pristine -Tascii and -Tutf8 both close the detached C D row at br,
    // without any new empty row before Y. The fallback has no accepted
    // native loop row independent of the existing HEAD projection.
    let source = r#".Bl -inset
.It Em "\X'tty: link https://k.test'C\X'tty: link' D\p" Sy "\h'-6n'\&"
.No "\p"
.br
.No "\h'-1n'Y"
.El
"#;
    assert_eq!(rows(source), ["C D", "Y"]);
}

#[test]
fn paragraph_spacing_keeps_an_independent_accepted_blank_before_its_own_row() {
    // term_fill ends the authored HEAD pass; term_flushln's tail closes
    // the already-open empty row; term_vspace then emits a separate row.
    // The two controls differ in native flag side effects, not blank count.
    for request in [".Pp", ".sp 1"] {
        let source = format!(".Bl -inset\n.It \"A\\p \\p \"\n{request}\nC\n.El\n");
        assert_eq!(rows(&source), ["A", "", "", "C"], "{source}");
        let source = format!(".Bl -inset\n.It A\n{request}\nC\n.El\n");
        assert_eq!(rows(&source), ["A", "", "C"], "{source}");
    }
    assert_eq!(
        rows(".Bl -inset\n.It \"A\\p \\p \"\n.br\nC\n.El\n"),
        ["A", "", "C"]
    );
}
