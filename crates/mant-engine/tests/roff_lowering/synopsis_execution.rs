//! Synopsis source phases share one live buffer across output owners.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{
    Block, Inline, LinkTarget, ListKind, ResolvedContent,
    visit::{self, Visit},
};
use serde::Deserialize;

use super::case_failures::verify_cases;
use super::json_roundtrip::json_string_roundtrip;

#[path = "synopsis_execution/query.rs"]
mod query;
#[path = "synopsis_execution/word_observations.rs"]
mod word_observations;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    core_count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    source_sha256: String,
    oracle: std::collections::BTreeMap<String, word_observations::Profile>,
    #[serde(skip)]
    exact_word_observation: bool,
    cohort: String,
    rows: Vec<String>,
    reading_rows: Vec<String>,
    generated_reading_spaces: Vec<GeneratedReadingSpace>,
    owners: Vec<NativeOwner>,
    font_words: Vec<FontWord>,
    links: Vec<ManualLink>,
    portable_rows: Vec<String>,
    portable_owners: Vec<PortableOwner>,
    portable_font_words: Vec<FontWord>,
}

#[derive(Deserialize)]
struct GeneratedReadingSpace {
    row: usize,
    start: usize,
    count: usize,
}

fn assert_generated_reading_projection(case: &Case) {
    let mut projected = case.rows.clone();
    for word in &case.generated_reading_spaces {
        assert!(word.count > 0 && word.count <= 2);
        let mut scalars: Vec<_> = projected[word.row].chars().collect();
        assert_eq!(
            &scalars[word.start..word.start + word.count],
            vec!['\u{a0}'; word.count],
            "{}: only the source-executed generated escaped word maps to reading SP",
            case.id
        );
        scalars[word.start..word.start + word.count].fill(' ');
        projected[word.row] = scalars.into_iter().collect();
    }
    assert_eq!(projected, case.reading_rows, "{}: reading policy", case.id);
}

#[derive(Deserialize)]
struct PortableOwner {
    kind: String,
    rows: Vec<String>,
    line: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct NativeOwner {
    r#macro: String,
    kind: String,
    line: u32,
    column: u32,
    line_start: bool,
    no_print: bool,
}

#[derive(Deserialize)]
struct FontWord {
    text: String,
    styles: Vec<u8>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ManualLink {
    label: String,
    name: String,
    section: String,
}

fn cases() -> Vec<Case> {
    let mut fixture: Fixture =
        serde_json::from_str(include_str!("synopsis_execution/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.core_count, 96);
    assert_eq!(fixture.cases.len(), 146);
    assert_eq!(
        fixture
            .cases
            .iter()
            .filter(|case| case.cohort == "core")
            .count(),
        96
    );
    word_observations::apply(&mut fixture.cases);
    fixture.cases
}

fn native_owners(node: &Node, output: &mut Vec<NativeOwner>) {
    if let Some(token) = node.macro_token.as_deref()
        && matches!(token, "Nm" | "Vt" | "Ft" | "Fd" | "Cd" | "In" | "Fn" | "Fo")
        && node.line > 3
    {
        let kind = match node.kind {
            NodeKind::Element => "elem",
            NodeKind::Block => "block",
            NodeKind::Head => "head",
            NodeKind::Body => "body",
            _ => panic!("unexpected synopsis native role"),
        };
        output.push(NativeOwner {
            r#macro: token.into(),
            kind: kind.into(),
            line: node.line,
            column: node.column,
            line_start: node.flags.line_start,
            no_print: node.flags.no_print,
        });
    }
    for child in &node.children {
        native_owners(child, output);
    }
}

fn roundtrip(case: &Case) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    let decoded = json_string_roundtrip(&content);
    assert!(
        mant_ir::validate_document(decoded.document.as_ref().unwrap()).is_empty(),
        "{}: valid owned IR",
        case.id
    );
    decoded
}

fn body_rows(text: &str) -> Vec<String> {
    let body = text
        .split_once("SYNOPSIS\n")
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0;
    // Only responsive device origin and terminal row padding are excluded.
    // In particular, no split_whitespace: two SP, NBSP, TAB, omitted glyphs
    // and every internal empty/hard row remain distinguishable.
    body.trim_end_matches('\n')
        .split('\n')
        .map(|row| row.trim_matches(' ').to_owned())
        .collect()
}

fn rows(content: &ResolvedContent) -> Vec<String> {
    body_rows(&mant_render::render_query_man(content))
}

fn observed_rows(content: &ResolvedContent, case: &Case) -> Vec<String> {
    if !case.exact_word_observation {
        return rows(content);
    }
    let text = mant_render::render_query_man(content);
    text.split_once("SYNOPSIS\n")
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0
        .trim_end_matches('\n')
        .split('\n')
        .map(|row| row.trim_end_matches(' ').to_owned())
        .collect()
}

#[derive(Default)]
struct ReadingFacts {
    style: u8,
    scalars: Vec<(char, u8)>,
    links: Vec<ManualLink>,
}

impl<'ir> Visit<'ir> for ReadingFacts {
    fn visit_heading(&mut self, _: &'ir mant_ir::Heading) {}

    fn visit_block(&mut self, block: &'ir Block) {
        visit::walk_block(self, block);
        self.scalars.push(('\n', 0));
    }

    fn visit_inline(&mut self, inline: &'ir Inline) {
        let saved = self.style;
        match inline {
            Inline::Strong { .. } => self.style |= 1,
            Inline::Emphasis { .. } => self.style |= 2,
            Inline::Code { value } => {
                self.scalars
                    .extend(value.chars().map(|ch| (ch, self.style | 4)));
            }
            Inline::Text { value } | Inline::Equation { value, .. } => {
                self.scalars
                    .extend(value.chars().map(|ch| (ch, self.style)));
            }
            Inline::LineBreak {} => self.scalars.push(('\n', self.style)),
            Inline::Link {
                target:
                    LinkTarget::Manual {
                        name,
                        manual_section,
                    },
                children,
                ..
            } => self.links.push(ManualLink {
                label: mant_ir::inline_plain_text(children),
                name: name.clone(),
                section: manual_section.clone().unwrap(),
            }),
            _ => {}
        }
        visit::walk_inline(self, inline);
        self.style = saved;
    }
}

fn assert_reading_facts(
    content: &ResolvedContent,
    case: &Case,
    font_words: &[FontWord],
    expected_links: &[ManualLink],
) {
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SYNOPSIS")
        .unwrap();
    let mut facts = ReadingFacts::default();
    facts.visit_section(section);
    assert_eq!(
        facts.links, expected_links,
        "{}: exact labels/target occurrences",
        case.id
    );
    let text: String = facts.scalars.iter().map(|(ch, _)| ch).collect();
    for word in font_words {
        let mut found = Vec::new();
        for (byte, _) in text.match_indices(&word.text) {
            let before = text[..byte].chars().next_back();
            let after = text[byte + word.text.len()..].chars().next();
            if before.is_some_and(char::is_alphabetic) || after.is_some_and(char::is_alphabetic) {
                continue;
            }
            let start = text[..byte].chars().count();
            let styles = &facts.scalars[start..start + word.text.chars().count()];
            assert!(
                styles.iter().all(|(_, style)| *style == styles[0].1),
                "{}: split {} font",
                case.id,
                word.text
            );
            found.push(styles[0].1);
        }
        assert_eq!(
            found, word.styles,
            "{}: {} font and occurrence count",
            case.id, word.text
        );
    }
}

fn assert_portable_owners(content: &ResolvedContent, case: &Case) {
    if case.portable_owners.is_empty() {
        return;
    }
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SYNOPSIS")
        .unwrap();
    assert_eq!(
        section.blocks.len(),
        case.portable_owners.len(),
        "{}: original declaration owners",
        case.id
    );
    for (block, expected) in section.blocks.iter().zip(&case.portable_owners) {
        assert_eq!(expected.kind, "paragraph");
        let Block::Paragraph {
            children, source, ..
        } = block
        else {
            panic!("{}: original paragraph owner", case.id);
        };
        assert_eq!(
            source.unwrap().line,
            expected.line,
            "{}: original owner source",
            case.id
        );
        assert_eq!(
            mant_ir::inline_plain_text(children),
            expected.rows.join("\n"),
            "{}: original owner content/hard rows",
            case.id
        );
    }
}

fn verify_all_cases(check: impl Fn(&Case) + std::panic::RefUnwindSafe) {
    verify_cases(cases(), |case| &case.id, check);
}

// Canonical Markdown renders a definition as a bullet item. Read that real
// item owner, retaining its blocks and hard breaks; never erase bullet bytes
// from the output string or concatenate text across block boundaries.
fn unwrap_exported_item(blocks: &mut Vec<Block>) {
    let mut output = Vec::new();
    for block in std::mem::take(blocks) {
        if let Block::List {
            kind: ListKind::Bullet,
            mut items,
            ..
        } = block
        {
            assert_eq!(items.len(), 1);
            let mut item = items.remove(0);
            unwrap_exported_item(&mut item.blocks);
            output.extend(item.blocks);
        } else {
            output.push(block);
        }
    }
    *blocks = output;
}

#[test]
fn synopsis_core_and_pairwise_phases_match_native_after_json() {
    // Each complete source ran pinned ASCII/UTF-8(78)/HTML/tree/lint before
    // these expectations. Lint status/messages remain in the fixture: core
    // lacks NAME (warning); deliberately empty Nm/Fn are recovery controls.
    // CVS mdoc_term.c:1185-1264,1338-1438,1552-1577,1750-1804 owns the
    // macro pre/children/post. term.c:473-481 only conditionally flushes;
    // Fn/Fo's term_flushln is unconditional and clears BACK* only there.
    verify_all_cases(|case| {
        assert_generated_reading_projection(case);
        let parsed = Parser::default()
            .parse_bytes(&case.id, case.source.as_bytes())
            .unwrap();
        let mut actual = Vec::new();
        native_owners(&parsed.document.root, &mut actual);
        assert_eq!(
            actual, case.owners,
            "{}: native roles/source/entry flags",
            case.id
        );
        let content = roundtrip(case);
        assert_eq!(
            observed_rows(&content, case),
            case.reading_rows,
            "{}: exact hard rows and word scalars",
            case.id
        );
        assert_portable_owners(&content, case);
        assert_reading_facts(&content, case, &case.font_words, &case.links);
    });
}

#[test]
fn synopsis_actual_markdown_parser_preserves_rows_styles_and_link_owners() {
    verify_all_cases(|case| {
        let content = roundtrip(case);
        assert_portable_owners(&content, case);
        let markdown = mant_codec::encode::render_markdown(&content);
        let mut read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let section = read
            .document
            .as_mut()
            .unwrap()
            .sections
            .iter_mut()
            .find(|section| section.heading.plain_text() == "SYNOPSIS")
            .unwrap();
        if case.source.contains(".It") {
            unwrap_exported_item(&mut section.blocks);
        }
        assert_eq!(
            observed_rows(&read, case),
            case.portable_rows,
            "{}: actual Markdown body rows\n{markdown}",
            case.id
        );
        // Default body Markdown intentionally renders Manual links as
        // transparent labels. JSON, outline and original-IR consumer tests
        // above retain the exact typed targets; this readback does not claim
        // to serialize those targets through the portable body policy.
        assert_reading_facts(&read, case, &case.portable_font_words, &[]);
        if case.exact_word_observation {
            let markdown = mant_codec::encode::render_markdown(&read);
            let second = mant_loader::load_markdown_text(&markdown, None).unwrap();
            assert_eq!(
                observed_rows(&second, case),
                case.portable_rows,
                "{}: second source-bound Markdown readback\n{markdown}",
                case.id
            );
            assert_reading_facts(&second, case, &case.portable_font_words, &[]);
        }
    });
}

#[test]
fn row_projection_rejects_internal_space_glyph_and_hard_row_mutations() {
    let project = |body: &str| body_rows(&format!("SYNOPSIS\n{body}\n\nNEXT\nAFTER.\n"));
    assert_eq!(project("   A  B\n\n C"), ["A  B", "", "C"]);
    let expected = project("A  B\n\nC");
    for mutation in ["A B\n\nC", "A B\nC", "A  B C", "A  B\n\n", "  B\n\nC"] {
        assert_ne!(project(mutation), expected, "{mutation:?}");
    }
    assert_ne!(project("A B"), project("A\u{a0}B"));
}
