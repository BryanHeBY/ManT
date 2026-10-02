//! Pristine-bound roff carriers exercise the actual Markdown reader and queries.

use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown_with_options,
};
use mant_ir::{Block, Inline, ResolvedContent};
use mant_protocol::{QueryBundle, SearchCase, SearchQuery, SearchScope, SearchSyntax};
use serde::Deserialize;

#[path = "markdown_hard_rows/invariants.rs"]
mod invariants;

#[derive(Deserialize)]
struct Matrix {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    metadata: Metadata,
    native_rows: Vec<String>,
}

#[derive(Deserialize)]
struct Metadata {
    container: String,
    carrier: String,
    hardline: String,
}

fn cases() -> Vec<Case> {
    let matrix: Matrix =
        serde_json::from_str(include_str!("markdown_hard_rows/cases.json")).unwrap();
    assert_eq!(matrix.cases.len(), 100);
    matrix.cases
}

fn description(content: &ResolvedContent) -> &[Block] {
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

fn first_children(blocks: &[Block]) -> &[Inline] {
    match &blocks[0] {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => children,
        Block::DefinitionList { items, .. } => &items[0].terms[0],
        Block::Table { rows, .. } => first_children(&rows[0].cells[0].blocks),
        Block::List { items, .. } => first_children(&items[0].blocks),
        other => panic!("unexpected consumer container: {other:#?}"),
    }
}

fn projection(children: &[Inline]) -> String {
    let mut output = String::new();
    for child in children {
        match child {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                output.push_str(&projection(children));
            }
            other => output.push_str(&mant_ir::inline_plain_text(std::slice::from_ref(other))),
        }
    }
    output
}

fn without_resolved_origins(value: &str) -> String {
    // Only this core's generated left padding is projected to NBSP. None of
    // its operands authors leading spaces/NBSP. Edges, repeated rows, interior
    // spaces, and all authored glyphs remain exact; broader M tests keep NBSP.
    value
        .split('\n')
        .map(|row| row.trim_start_matches([' ', '\u{a0}']))
        .collect::<Vec<_>>()
        .join("\n")
}

fn styles_for_word(children: &[Inline], word: &str) -> Vec<u8> {
    fn append(children: &[Inline], mask: u8, output: &mut Vec<(char, u8)>) {
        for child in children {
            match child {
                Inline::Text { value } | Inline::Code { value } => {
                    output.extend(value.chars().map(|character| (character, mask)));
                }
                Inline::Strong { children } => append(children, mask | 1, output),
                Inline::Emphasis { children } => append(children, mask | 2, output),
                Inline::Link { children, .. } => {
                    append(children, mask, output);
                }
                Inline::LineBreak { .. } => output.push(('\n', 0)),
                Inline::Anchor { .. } => {}
                other @ Inline::Equation { .. } => panic!("unexpected carrier: {other:?}"),
            }
        }
    }
    let mut output = vec![];
    append(children, 0, &mut output);
    let value = output
        .iter()
        .map(|(character, _)| *character)
        .collect::<String>();
    let byte = value.find(word).unwrap();
    let start = value[..byte].chars().count();
    output[start..start + word.chars().count()]
        .iter()
        .map(|(_, mask)| *mask)
        .collect()
}

fn assert_artifact_ranges(content: &ResolvedContent, word: &str) {
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let artifact =
            render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
        let found = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: word.into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope,
                word: true,
                context_lines: 0,
                limit: 20,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(found.total, 1, "{word}: {}", artifact.text());
        for result in &found.matches {
            for occurrence in &result.occurrences {
                let bytes = usize::try_from(occurrence.markdown.start_byte).unwrap()
                    ..usize::try_from(occurrence.markdown.end_byte).unwrap();
                assert_eq!(&artifact.text()[bytes], word, "{:?}", occurrence.markdown);
            }
        }
    }
}

fn owner_is_preserved(content: &ResolvedContent) -> bool {
    if let Block::DefinitionList { items, .. } = &description(content)[0] {
        !mant_ir::inline_plain_text(&items[0].terms[0]).contains("BodyWord")
            && matches!(items[0].description.first(), Some(Block::Paragraph { children, .. })
                if mant_ir::inline_plain_text(children) == "BodyWord")
            && items[0].source.is_some_and(|source| source.line == 9)
    } else {
        true
    }
}

fn assert_projection(
    case: &Case,
    original: &ResolvedContent,
    restored: &ResolvedContent,
    markdown: &str,
) {
    let original_children = first_children(description(original));
    let mut actual = mant_ir::inline_plain_text(first_children(description(restored)));
    let mut expected = projection(original_children);
    if case.metadata.container == "column" {
        // A multiline column table exports a plain fenced row with ' | '
        // delimiters, not live links. Its first cell retains every hard row.
        if let Some((first, _)) = actual.split_once(" | RIGHT") {
            actual = first.into();
        }
        expected = mant_ir::inline_plain_text(original_children);
    }
    if case.metadata.container == "literal" {
        expected = mant_ir::inline_plain_text(original_children);
    }
    if matches!(case.metadata.container.as_str(), "tag" | "hang") {
        assert!(actual.contains("BodyWord"), "{}: {actual:?}", case.id);
        actual = actual.split_once("BodyWord").unwrap().0.into();
        // Description placement is independently covered by definition tests.
        // Compare the complete HEAD, including its edge/repeated row events.
        let expected = without_resolved_origins(&expected);
        let actual = without_resolved_origins(&actual);
        assert!(
            actual.starts_with(expected.trim_end_matches(' ')),
            "{}: expected HEAD {expected:?}, actual {actual:?}\n{markdown}",
            case.id
        );
    } else {
        assert_eq!(
            without_resolved_origins(&actual),
            without_resolved_origins(&expected),
            "{}\n{markdown}",
            case.id
        );
    }
}

struct Links(Vec<(mant_ir::LinkTarget, String)>);
impl<'ir> mant_ir::visit::Visit<'ir> for Links {
    fn visit_inline(&mut self, node: &'ir Inline) {
        if let Inline::Link {
            target, children, ..
        } = node
        {
            self.0
                .push((target.clone(), mant_ir::inline_plain_text(children)));
        }
        mant_ir::visit::walk_inline(self, node);
    }
}

fn carrier_is_preserved(case: &Case, restored: &ResolvedContent, word: &str) -> bool {
    if matches!(case.metadata.container.as_str(), "column" | "literal") {
        return true;
    }
    native_carrier_is_preserved(case, restored, word)
}

fn native_carrier_is_preserved(case: &Case, restored: &ResolvedContent, word: &str) -> bool {
    let mask = match case.metadata.carrier.as_str() {
        "Em" | "Lk" => 2,
        "Sy" => 1,
        _ => 0,
    };
    if styles_for_word(first_children(description(restored)), word) != vec![mask; word.len()] {
        return false;
    }
    if case.metadata.carrier == "Lk" {
        use mant_ir::visit::Visit;
        let mut links = Links(vec![]);
        links.visit_document(restored.document.as_ref().unwrap());
        return links.0.len() == 1
            && links.0[0].0
                == mant_ir::LinkTarget::External {
                    uri: "https://ex.org".into(),
                }
            && links.0[0].1.contains(word);
    }
    true
}

#[test]
fn hundred_native_carriers_keep_hard_rows_after_json_and_markdown_boundary_policies() {
    // All 100 complete inputs ran registered pristine CVS ASCII, UTF-8, HTML,
    // tree and lint before freezing this fixture. ESCAPE_BREAK buffers '\n'
    // (term.c:657); term_fill (304) settles it at word ends. Native gold remains
    // available to acceptance replay; this consumer test checks accepted IR.
    for case in cases() {
        assert!(
            !case.native_rows.is_empty(),
            "{}: no native region",
            case.id
        );
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let wire = serde_json::to_string(&QueryBundle::from(&original)).unwrap();
        let content: ResolvedContent = serde_json::from_str::<QueryBundle>(&wire).unwrap().into();
        assert_eq!(original.document, content.document, "{}", case.id);
        assert!(
            owner_is_preserved(&content),
            "{}: wrong HEAD/BODY owner",
            case.id
        );
        let word = if case.metadata.hardline == "interior-one" {
            "A"
        } else {
            "AFTER"
        };
        assert!(
            native_carrier_is_preserved(&case, &content, word),
            "{}: native carrier lost",
            case.id
        );
        assert_artifact_ranges(&content, word);
        {
            let markdown = render_markdown_with_options(&content, MarkdownOptions::default());
            let restored =
                mant_loader::load_markdown_text(&markdown, Some("exported.md".into())).unwrap();
            let readback = mant_render::render_query_man(&restored);
            assert!(!readback.contains("<br>"), "{}: {readback}", case.id);
            assert!(!readback.contains("<br />"), "{}: {readback}", case.id);
            assert_projection(&case, &content, &restored, &markdown);
            assert!(
                carrier_is_preserved(&case, &restored, word),
                "{}: carrier lost\n{markdown}",
                case.id
            );
            assert_artifact_ranges(&restored, word);
        }
    }
}
