//! Executed equation suffixes retain author characters and native content owners.

use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown_with_options,
};
use mant_ir::{
    Block, DefinitionItem, Document, EntryOwner, Inline, ResolvedContent, inline_plain_text,
    visit::{self, Visit},
};
use mant_protocol::{SearchCase, SearchQuery, SearchRenderMath, SearchScope, SearchSyntax};
use pulldown_cmark::{Event, Options, Parser};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Matrix {
    reference_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    id: String,
    source: String,
    quotes: usize,
    apostrophes: usize,
    equations: usize,
    owner: Owner,
    body: String,
    #[serde(default)]
    term: Option<String>,
    #[serde(default)]
    link_label: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Owner {
    Paragraph,
    Preformatted,
}

fn cases() -> Vec<Case> {
    // Each exact source ran pristine ASCII, UTF-8, HTML, tree and lint before
    // these facts were written. roff.c::roff_eqndelim protects the ordinary
    // suffix with \&. roff_getarg consumes only quotes that it actually parses
    // as argument syntax. man/mdoc_term's EQ post uses the next NODE_LINE to
    // determine word separation. eqn_term.c::term_eqn clears NOSPACE after an
    // equation even when explicit .EN leaves no protected suffix text. A later
    // NODE_LINE word or equation therefore has an ordinary leading separator.
    // term.c::term_word also clears NONEWLINE for every actual word, including
    // a blank equation atom, while an empty equation leaves continuation set.
    // Literal owners omit blank row tails while preserving these row breaks.
    // Native HEAD/BODY ownership remains distinct.
    let matrix: Matrix = serde_json::from_str(include_str!("equation_quotes/cases.json")).unwrap();
    assert_eq!(
        matrix.reference_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(matrix.cases.len(), 54);
    matrix.cases
}

#[derive(Default)]
struct Inventory<'a> {
    paragraphs: Vec<&'a [Inline]>,
    literals: Vec<&'a [Inline]>,
    definitions: Vec<&'a DefinitionItem>,
    links: Vec<&'a [Inline]>,
    inline_equations: usize,
    block_equations: usize,
}

impl<'a> Visit<'a> for Inventory<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        match block {
            Block::Paragraph { children, .. } => self.paragraphs.push(children),
            Block::Preformatted { children, .. } => self.literals.push(children),
            Block::Equation { .. } => self.block_equations += 1,
            _ => {}
        }
        visit::walk_block(self, block);
    }

    fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
        self.definitions.push(item);
        visit::walk_definition_item(self, item);
    }

    fn visit_inline(&mut self, inline: &'a Inline) {
        match inline {
            Inline::Equation { value, expression } => {
                self.inline_equations += 1;
                assert_eq!(value, &expression.readable_text());
            }
            Inline::Link { children, .. } => self.links.push(children),
            _ => {}
        }
        visit::walk_inline(self, inline);
    }
}

fn observe(document: &Document) -> Inventory<'_> {
    let mut inventory = Inventory::default();
    inventory.visit_document(document);
    inventory
}

fn author_scalars(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn assert_characters(actual: &str, case: &Case) {
    assert_eq!(
        actual.matches('"').count(),
        case.quotes,
        "{}: {actual:?}",
        case.id
    );
    assert_eq!(
        actual.matches('\'').count(),
        case.apostrophes,
        "{}: {actual:?}",
        case.id
    );
}

fn markdown_visible(markdown: &str) -> String {
    // Decode CommonMark containers before counting. HTML attribute quotes and
    // nonvisible anchor declarations cannot satisfy an authored quote finding.
    Parser::new_ext(markdown, Options::ENABLE_MATH)
        .filter_map(|event| match event {
            Event::Text(value)
            | Event::Code(value)
            | Event::InlineMath(value)
            | Event::DisplayMath(value) => Some(value.into_string()),
            Event::SoftBreak | Event::HardBreak => Some("\n".into()),
            _ => None,
        })
        .collect()
}

fn assert_visible_search(content: &ResolvedContent, case: &Case) {
    let query = SearchQuery {
        pattern: "\"".into(),
        syntax: SearchSyntax::Literal,
        case: SearchCase::Sensitive,
        scope: SearchScope::Visible,
        word: false,
        context_lines: 0,
        limit: 100,
        offset: 0,
    };
    let result = mant_query::search_query(content, &query).unwrap();
    assert_eq!(result.render.math, SearchRenderMath::Readable);
    assert!(!result.truncated);
    let artifact =
        render_addressable_markdown_with_options(content, MarkdownOptions::READABLE_ADDRESSABLE);
    let occurrences: Vec<_> = result
        .matches
        .iter()
        .flat_map(|hit| &hit.occurrences)
        .collect();
    assert_eq!(
        occurrences.len(),
        case.quotes,
        "{}: visible search",
        case.id
    );
    for hit in &result.matches {
        assert!(!hit.occurrences_truncated);
        assert_eq!(
            usize::try_from(hit.occurrence_count).unwrap(),
            hit.occurrences.len()
        );
    }
    for occurrence in occurrences {
        assert_eq!(occurrence.matched_text, "\"");
        let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
        let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
        assert_eq!(artifact.text().get(start..end), Some("\""));
    }
}

fn assert_content(content: &ResolvedContent, case: &Case) {
    let document = content.document.as_ref().unwrap();
    assert_eq!(mant_ir::validate_document(document), Vec::new());
    let inventory = observe(document);
    assert_eq!(inventory.inline_equations, case.equations, "{}", case.id);
    assert_eq!(
        inventory.block_equations, 0,
        "{}: inline placement",
        case.id
    );
    let containers = match case.owner {
        Owner::Paragraph => &inventory.paragraphs,
        Owner::Preformatted => &inventory.literals,
    };
    let [owner] = containers.as_slice() else {
        panic!("{}: one complete prose or literal owner", case.id);
    };
    let visible = inline_plain_text(owner);
    assert_characters(&visible, case);
    // Filled source has protected blanks and responsive wrapping. Compare every
    // nonblank scalar as well as the separate exact quote counts and ownership.
    assert_eq!(
        author_scalars(&visible),
        author_scalars(&case.body),
        "{}",
        case.id
    );
    if case.term.is_none() {
        // These source-derived strings also distinguish adjacent x" from
        // authored x " and the ordinary separator before a later-line quote.
        // No-fill strings retain each exact physical row in the same owner.
        assert_eq!(
            visible, case.body,
            "{}: exact suffix spacing and physical rows",
            case.id
        );
    }
    if let Some(expected_label) = &case.link_label {
        let [label] = inventory.links.as_slice() else {
            panic!("{}: original link BODY", case.id);
        };
        assert_eq!(inline_plain_text(label), *expected_label);
        assert_characters(&inline_plain_text(label), case);
    }
    assert_characters(&mant_render::render_query_text(content), case);
    for options in [MarkdownOptions::default(), MarkdownOptions::READABLE] {
        let markdown = render_markdown_with_options(content, options);
        assert_characters(&markdown_visible(&markdown), case);
    }
    assert_visible_search(content, case);
}

#[test]
fn equation_quote_matrix_keeps_characters_owners_and_public_consumers() {
    super::case_failures::verify_cases(
        cases(),
        |case| &case.id,
        |case| {
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            assert_content(&original, case);
            let transported = super::json_roundtrip::json_string_roundtrip(&original);
            assert_eq!(transported, original);
            assert_content(&transported, case);
        },
    );
}

#[test]
fn equation_quotes_stay_in_native_definition_bodies_and_complete_forms() {
    let definitions = cases().into_iter().filter(|case| case.term.is_some());
    super::case_failures::verify_cases(
        definitions,
        |case| &case.id,
        |case| {
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let content = super::json_roundtrip::json_string_roundtrip(&original);
            let inventory = observe(content.document.as_ref().unwrap());
            let [item] = inventory.definitions.as_slice() else {
                panic!("{}: original definition owner", case.id);
            };
            let [term] = item.terms.as_slice() else {
                panic!("{}: one complete native term", case.id);
            };
            let expected_term = case.term.as_deref().unwrap();
            assert_eq!(
                author_scalars(&inline_plain_text(term)),
                author_scalars(expected_term)
            );
            let mut form_inventory = Inventory::default();
            for inline in term {
                form_inventory.visit_inline(inline);
            }
            assert_eq!(
                form_inventory.inline_equations, 0,
                "{}: native HEAD",
                case.id
            );
            assert_eq!(inline_plain_text(term).matches('"').count(), 0);
            let forms = EntryOwner::Definition(item)
                .forms()
                .expect("valid recorded forms");
            let projected: Vec<_> = forms.iter().map(inline_plain_text).collect();
            assert_eq!(projected.len(), 1, "{}: complete form", case.id);
            assert_eq!(author_scalars(&projected[0]), author_scalars(expected_term));
            let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
                panic!("{}: complete native BODY paragraph", case.id);
            };
            let text = inline_plain_text(children);
            assert_eq!(
                author_scalars(&text),
                author_scalars(&case.body),
                "{}",
                case.id
            );
            assert_characters(&text, case);
        },
    );
}
