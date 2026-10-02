//! A control request consumes one old native field before changing flags.

fn complete_source(body: &str) -> String {
    format!(
        ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n{body}.Sh NEXT\n.No END\n"
    )
}

fn description_rows(body: &str) -> (mant_ir::ResolvedContent, Vec<String>) {
    let query = mant_loader::load_roff_bytes(complete_source(body).as_bytes()).unwrap();
    let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    assert!(!json.contains("\\u0000mant:"), "private word owner escaped");
    let roundtrip: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let query: mant_ir::ResolvedContent = roundtrip.into();
    let text = mant_render::render_query_man(&query);
    let region = text
        .split_once("DESCRIPTION\n")
        .expect("DESCRIPTION owner")
        .1
        .split_once("NEXT\n")
        .expect("NEXT owner")
        .0;
    let mut rows: Vec<_> = region.split_terminator('\n').map(str::to_owned).collect();
    // Only NEXT's one section-spacing row is furniture. Preserve any other
    // leading, internal or trailing physical rows, including empty ones.
    assert_eq!(rows.pop().as_deref(), Some(""));
    (query, rows)
}

#[test]
fn pre_br_requests_share_the_original_field_row_and_word_receipt() {
    // These four exact sources ran registered pristine ASCII/UTF-8/HTML/
    // tree/lint before assertions. roff_term.c:69-78 calls term_newln with
    // old NOBREAK/BRIND; 233-236 reuses it for ti, including no operand.
    // term.c:475-481 selects NOSPACE first; 250-253 decides the actual tail.
    for (body, expected) in [
        (
            ".Bl -tag -width 8n\n.It Xo\n.No \"\\&\"\n.nf\n.fi\n.No AFTER\n.Xc\n.No BodyWord\n.El\n",
            vec!["AFTER", "          BodyWord"],
        ),
        (
            ".Bl -tag -width 8n\n.It Xo\n.No \"A\\c\"\n.nf\n.No MID\n.fi\n.No AFTER\n.Xc\n.No BodyWord\n.El\n",
            vec!["A         MID", "AFTER", "          BodyWord"],
        ),
        (
            ".Bl -hang -width 8n\n.It Xo\n.nf\n.No MID\n.fi\n.No AFTER\n.Xc\n.No BodyWord\n.El\n",
            vec!["          MIDAFTERBodyWord"],
        ),
        (
            ".Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No A\n.ti\n.No AFTER\n.Xc Ta RIGHT\n.El\n",
            vec!["A AFTER     RIGHT"],
        ),
    ] {
        let (_, rows) = description_rows(body);
        assert_eq!(rows, expected, "{body}");
    }
}

mod head_word_padding {
    use libmandoc_rs::{Node, NodeKind};
    use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
    use mant_ir::{Block, DefinitionItem, ResolvedContent};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Matrix {
        header: Header,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Header {
        count: usize,
        unique_sources: usize,
        expectations_from_product: bool,
        oracle_sha256: String,
    }

    #[derive(Deserialize)]
    struct Position {
        line: u32,
        column: u32,
    }

    #[derive(Deserialize)]
    struct Case {
        id: String,
        source: String,
        head_word_source: Position,
        body_word_source: Position,
        expected_rows: Vec<String>,
        expected_head_words: String,
    }

    fn cases() -> Vec<Case> {
        let matrix: Matrix = serde_json::from_str(include_str!(
            "control_request_matrix/head_word_padding_rows.json"
        ))
        .unwrap();
        assert_eq!(
            (matrix.header.count, matrix.header.unique_sources),
            (189, 187)
        );
        assert!(!matrix.header.expectations_from_product);
        assert_eq!(
            matrix.header.oracle_sha256,
            "482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6"
        );
        assert_eq!(matrix.cases.len(), matrix.header.count);
        matrix.cases
    }

    fn word_signature(text: &str) -> Vec<&str> {
        // Ordinary device padding is responsive. Keep Unicode spaces,
        // control scalars and the difference between joined/separate words.
        text.split([' ', '\t', '\n'])
            .filter(|word| !word.is_empty())
            .collect()
    }

    fn responsive_row(row: &str) -> String {
        word_signature(row).join(" ")
    }

    fn owns_position(
        node: &Node,
        mut owner: Option<NodeKind>,
        position: &Position,
        expected: NodeKind,
    ) -> bool {
        if node.macro_name.as_deref() == Some("It")
            && matches!(node.kind, NodeKind::Head | NodeKind::Body)
        {
            owner = Some(node.kind);
        }
        (node.kind == NodeKind::Text
            && node.line == position.line
            && node.column == position.column
            && owner == Some(expected))
            || node
                .children
                .iter()
                .any(|child| owns_position(child, owner, position, expected))
    }

    fn assert_ast_ownership(case: &Case) {
        let parsed = libmandoc_rs::Parser::default()
            .parse_bytes("head-word-padding.1", case.source.as_bytes())
            .unwrap();
        assert!(
            owns_position(
                &parsed.document.root,
                None,
                &case.head_word_source,
                NodeKind::Head
            ) && owns_position(
                &parsed.document.root,
                None,
                &case.body_word_source,
                NodeKind::Body
            ),
            "{}: the declared HEAD/BODY paths are unreachable",
            case.id
        );
    }

    fn roundtrip(case: &Case) -> ResolvedContent {
        let query = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{}: private owner escaped",
            case.id
        );
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        decoded.into()
    }

    fn description_blocks(content: &ResolvedContent) -> &[Block] {
        &content
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "DESCRIPTION")
            .unwrap()
            .blocks
    }

    fn description_region(rendered: &str) -> &str {
        rendered
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("NEXT\n")
            .unwrap()
            .0
    }

    fn definition(content: &ResolvedContent) -> &DefinitionItem {
        let Block::DefinitionList { items, .. } = &description_blocks(content)[0] else {
            panic!("the exact source lost its definition owner");
        };
        assert_eq!(items.len(), 1);
        &items[0]
    }

    fn check_native_rows(case: &Case, content: &ResolvedContent, failures: &mut Vec<String>) {
        let rendered = mant_render::render_query_man(content);
        let mut rows: Vec<_> = description_region(&rendered)
            .split_terminator('\n')
            .map(responsive_row)
            .collect();
        // NEXT's one section-spacing row is furniture. All other leading,
        // internal or trailing rows remain observable, including empty ones.
        assert_eq!(rows.pop().as_deref(), Some(""), "{}: NEXT spacing", case.id);
        let expected: Vec<_> = case
            .expected_rows
            .iter()
            .map(|row| responsive_row(row))
            .collect();
        if rows != expected {
            failures.push(format!(
                "{}: native rows {expected:?}, rendered {rows:?}",
                case.id
            ));
        }
    }

    fn check_head_words(case: &Case, content: &ResolvedContent, failures: &mut Vec<String>) {
        let item = definition(content);
        let head = item
            .terms
            .iter()
            .map(|term| mant_ir::inline_plain_text(term))
            .collect::<Vec<_>>()
            .join("\n");
        if word_signature(&head) != word_signature(&case.expected_head_words) {
            failures.push(format!(
                "{}: native HEAD {:?}, IR HEAD {head:?}",
                case.id, case.expected_head_words
            ));
        }
        if word_signature(&case.expected_head_words).first() == Some(&"X")
            && let Some(entry) = &item.entry
            && entry.names.iter().any(|name| {
                name.starts_with("Xabc") || name.starts_with("Xa--bc") || name.starts_with("Xa-bc")
            })
        {
            failures.push(format!(
                "{}: field padding loss manufactured entry names {:?}",
                case.id, entry.names
            ));
        }
    }

    fn append_reader_words(blocks: &[Block], output: &mut String) {
        for block in blocks {
            match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    output.push_str(&mant_ir::inline_plain_text(children));
                    output.push('\n');
                }
                Block::List { items, .. } => {
                    // Only container-generated markers are omitted. Every
                    // authored inline glyph (including '-' or '•') survives.
                    assert_eq!(items.len(), 1);
                    append_reader_words(&items[0].blocks, output);
                }
                Block::VerticalSpace { .. } => output.push('\n'),
                other => panic!("unexpected native Markdown reader owner: {other:?}"),
            }
        }
    }

    fn check_native_markdown(case: &Case, content: &ResolvedContent, failures: &mut Vec<String>) {
        let markdown = render_markdown_with_options(content, MarkdownOptions::default());
        let reader =
            mant_loader::load_markdown_text(&markdown, Some("head-padding.md".into())).unwrap();
        let mut readback = String::new();
        append_reader_words(description_blocks(&reader), &mut readback);
        let native = case.expected_rows.join("\n");
        if word_signature(&readback) != word_signature(&native) {
            failures.push(format!(
                "{}: native Markdown lost a field word seam: {readback:?}",
                case.id
            ));
        }
    }

    #[test]
    fn accepted_head_padding_survives_rejected_suffixes_and_semantic_owners() {
        // Every complete source ran the registered pristine ASCII/UTF-8/HTML/
        // tree/lint profiles before this fixture was written. term.c:113-116
        // positions minbl before the accepted graph; term_field():389-427 prints
        // that pad even if a later term_fill pass rejects the remaining field
        // (143-146,217,233-253). Padding is not a source owner's scalar quota.
        // The 189 identities include the five reported and eight adjacent cases,
        // TAG/HANG x 0/2/4n x six carriers x three marker states, Sm/Ns contrasts,
        // and styled owners on both sides of the real pre-br boundary.
        let mut failures = Vec::new();
        for case in cases() {
            assert_ast_ownership(&case);
            let content = roundtrip(&case);
            check_native_rows(&case, &content, &mut failures);
            check_head_words(&case, &content, &mut failures);
            check_native_markdown(&case, &content, &mut failures);
        }
        assert!(
            failures.is_empty(),
            "{} failures:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
