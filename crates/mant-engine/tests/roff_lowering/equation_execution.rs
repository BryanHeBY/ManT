//! Formula owners execute the same incoming word state as ordinary source text.

use std::collections::BTreeSet;

use mant_codec::encode::{
    MarkdownOptions, render_addressable_markdown_with_options, render_markdown_with_options,
};
use mant_ir::{
    Block, DefinitionItem, DiagnosticImpact, Document, EquationExpression, Inline, ResolvedContent,
    inline_plain_text,
    visit::{self, Visit},
};
use mant_protocol::{SearchCase, SearchQuery, SearchRenderMath, SearchScope, SearchSyntax};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
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
    axes: [String; 6],
    source: String,
    source_sha256: String,
    body: String,
    native_collision: bool,
    damaged_math: bool,
    #[serde(default)]
    native_raw_rows: Vec<String>,
}

impl Case {
    fn id(&self) -> &str {
        &self.id
    }

    fn generated_source(&self) -> String {
        let [macro_set, mode, prefix, payload, origin, suffix] = self.axes.each_ref();
        let header = match macro_set.as_str() {
            "man" => ".TH RECHECK 1 \"2026-11-09\"\n.SH DESCRIPTION\n",
            "mdoc" => ".Dd November 9, 2026\n.Dt RECHECK 1\n.Os\n.Sh DESCRIPTION\n",
            _ => panic!("unknown macro set"),
        };
        let mode = match mode.as_str() {
            "filled" => "",
            "literal" => ".nf\n",
            _ => panic!("unknown fill mode"),
        };
        let prefix = match prefix.as_str() {
            "plain" => "A",
            "continue" => "A\\c",
            "hard-break" => "A\\p",
            "zero-bare" => "A\\z",
            "zero-X" => "A\\zX",
            _ => panic!("unknown incoming word state"),
        };
        let payload = match payload.as_str() {
            "one-atom" => "x",
            "two-atoms" => "x y",
            "empty-root" => "",
            "empty-atom" => "\"\"",
            "space-atom" => "\" \"",
            "two-space-atom" => "\"  \"",
            _ => panic!("unknown equation payload"),
        };
        let origin = match origin.as_str() {
            "same-line" => "",
            "new-line" => "\n",
            _ => panic!("unknown equation source boundary"),
        };
        let suffix = match suffix.as_str() {
            "same-line" => "B",
            "new-line" => "\nB",
            "adjacent-equation" => "$y$B",
            _ => panic!("unknown equation suffix"),
        };
        format!("{header}.EQ\ndelim $$\n.EN\n{mode}{prefix}{origin}${payload}${suffix}\nC\n")
    }
}

fn cases() -> Vec<Case> {
    // Every exact generated source ran pristine ASCII, UTF-8, HTML, tree and
    // lint before these source-bound facts were recorded. Native term_word
    // consumes pending \p, \z and NONEWLINE at each real word, including a
    // quoted empty or blank atom. eqn_term.c::eqn_box supplies that ordered
    // stream, and term_eqn clears NOSPACE once after the complete equation.
    // roff.c::roff_eqndelim inserts protected \& on same-line boundaries.
    let matrix: Matrix =
        serde_json::from_str(include_str!("equation_execution/matrix.json")).unwrap();
    assert_eq!(
        matrix.reference_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(matrix.cases.len(), 720);
    let mut ids = BTreeSet::new();
    for case in &matrix.cases {
        assert!(ids.insert(case.id().to_owned()));
        assert_eq!(case.id, case.axes.join("--"));
        assert_eq!(case.source, case.generated_source(), "{}", case.id());
        assert_eq!(case.source_sha256.len(), 64);
        assert_eq!(
            case.native_collision || case.damaged_math,
            !case.native_raw_rows.is_empty()
        );
    }
    matrix.cases
}

#[derive(Default)]
struct Inventory<'a> {
    paragraphs: Vec<&'a [Inline]>,
    literals: Vec<&'a [Inline]>,
    equations: Vec<&'a str>,
    expressions: Vec<&'a EquationExpression>,
    definitions: Vec<&'a DefinitionItem>,
    links: Vec<&'a [Inline]>,
    displays: usize,
}

impl<'a> Visit<'a> for Inventory<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        match block {
            Block::Paragraph { children, .. } => self.paragraphs.push(children),
            Block::Preformatted { children, .. } => self.literals.push(children),
            Block::Equation { .. } => self.displays += 1,
            _ => {}
        }
        visit::walk_block(self, block);
    }

    fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
        self.definitions.push(item);
        visit::walk_definition_item(self, item);
    }

    fn visit_inline(&mut self, inline: &'a Inline) {
        if let Inline::Equation { value, expression } = inline {
            assert_eq!(value, &expression.readable_text());
            self.equations.push(value);
            self.expressions.push(expression);
        }
        if let Inline::Link { children, .. } = inline {
            self.links.push(children);
        }
        visit::walk_inline(self, inline);
    }
}

fn observe(document: &Document) -> Inventory<'_> {
    let mut result = Inventory::default();
    result.visit_document(document);
    result
}

fn text_body(content: &ResolvedContent) -> String {
    let text = mant_render::render_query_text(content);
    let (_, body) = text.split_once("DESCRIPTION\n").expect("source section");
    body.strip_prefix('\n').unwrap_or(body).to_owned()
}

fn simple_math_reading(tex: &str) -> String {
    // This finite matrix contains only x/y atoms and literal spaces. Decode
    // exactly their fixed font/text wrappers, with every authored blank kept.
    // More complex shapes have independent structural assertions below.
    let decoded = tex
        .replace("\\mathit", "")
        .replace("\\mathord", "")
        .replace("\\textit", "")
        .replace("\\mathrm", "")
        .replace("\\text", "")
        .replace(['{', '}'], "")
        .replace("\\ ", " ");
    assert!(
        decoded.chars().all(|c| matches!(c, 'x' | 'y' | ' ')),
        "unexpected finite-matrix LaTeX body: {tex:?}"
    );
    decoded
}

fn markdown_body(markdown: &str, masked_math: Option<&str>) -> String {
    let mut heading = false;
    let mut heading_text = String::new();
    let mut body = false;
    let mut output = String::new();
    let mut fenced = false;
    let mut paragraph_ended = false;
    for event in Parser::new_ext(markdown, Options::ENABLE_MATH) {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                heading = true;
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                heading = false;
                body = heading_text == "DESCRIPTION";
            }
            Event::Text(value) if heading => heading_text.push_str(&value),
            Event::Start(Tag::Paragraph) if body => {
                if paragraph_ended && !output.is_empty() {
                    output.push_str("\n\n");
                }
                paragraph_ended = false;
            }
            Event::End(TagEnd::Paragraph) if body => paragraph_ended = true,
            Event::Start(Tag::CodeBlock(_)) if body => fenced = true,
            Event::End(TagEnd::CodeBlock) if body && fenced => {
                assert!(output.ends_with('\n'));
                output.pop(); // Markdown fence syntax terminates its payload row.
                fenced = false;
            }
            Event::Code(value) if body && masked_math == Some(value.as_ref()) => {
                output.push_str("<MATH>");
            }
            Event::Text(value) | Event::Code(value) if body => output.push_str(&value),
            Event::InlineMath(value) if body && masked_math.is_some() => {
                assert!(!value.is_empty());
                output.push_str("<MATH>");
            }
            Event::InlineMath(value) if body => output.push_str(&simple_math_reading(&value)),
            Event::SoftBreak | Event::HardBreak if body => output.push('\n'),
            _ => {}
        }
    }
    output
}

fn assert_search(content: &ResolvedContent, expected: &str) {
    let artifact =
        render_addressable_markdown_with_options(content, MarkdownOptions::READABLE_ADDRESSABLE);
    let visible_characters: Vec<_> =
        mant_codec::markdown_mapping::markdown_source_events(artifact.text())
            .flat_map(|(event, range)| {
                use mant_codec::markdown_mapping::{InlineMappingKind, map_inline_characters};
                match event {
                    Event::Text(value) => map_inline_characters(
                        artifact.text(),
                        &value,
                        range,
                        InlineMappingKind::Text,
                    ),
                    Event::Code(value) => map_inline_characters(
                        artifact.text(),
                        &value,
                        range,
                        InlineMappingKind::Code,
                    ),
                    _ => Vec::new(),
                }
            })
            .collect();
    for pattern in ["X", "AB", "A B", "A xB", "AxB"] {
        let request = SearchQuery {
            pattern: pattern.into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 100,
            offset: 0,
        };
        let result = mant_query::search_query(content, &request).unwrap();
        assert_eq!(result.render.math, SearchRenderMath::Readable);
        assert!(!result.truncated);
        let actual: Vec<_> = result
            .matches
            .iter()
            .flat_map(|hit| &hit.occurrences)
            .collect();
        assert_eq!(
            actual.len(),
            expected
                .lines()
                .map(|line| line.matches(pattern).count())
                .sum::<usize>(),
            "visible literal search {pattern:?} in {expected:?}"
        );
        for occurrence in actual {
            assert_eq!(occurrence.matched_text, pattern);
            let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
            let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
            assert!(artifact.text().get(start..end).is_some());
            let visible: String = visible_characters
                .iter()
                .filter(|character| character.source.start < end && start < character.source.end)
                .map(|character| character.value)
                .collect();
            assert_eq!(
                visible, pattern,
                "source range covers the exact visible match across Markdown syntax"
            );
        }
    }
}

fn assert_case(content: &ResolvedContent, case: &Case) {
    let document = content.document.as_ref().unwrap();
    assert_eq!(mant_ir::validate_document(document), Vec::new());
    let inventory = observe(document);
    assert_eq!(inventory.displays, 0, "delimiter equations stay inline");
    let owners = if case.axes[1] == "literal" {
        assert_eq!(inventory.paragraphs.len(), 0);
        &inventory.literals
    } else {
        assert_eq!(inventory.literals.len(), 0);
        &inventory.paragraphs
    };
    let [owner] = owners.as_slice() else {
        panic!("{}: one complete paragraph or literal owner", case.id());
    };
    assert_eq!(
        inline_plain_text(owner),
        case.body,
        "exact source characters, blanks and physical rows"
    );
    if !case.damaged_math {
        let payload = case.axes[3].as_str();
        let normal = matches!(payload, "one-atom" | "two-atoms");
        if normal || payload == "empty-root" {
            let mut expected_equations = Vec::new();
            if normal {
                expected_equations.push(if payload == "one-atom" { "x" } else { "x y" });
            }
            if case.axes[5] == "adjacent-equation" {
                expected_equations.push("y");
            }
            assert_eq!(
                inventory.equations, expected_equations,
                "one complete identity per undamaged mathematical owner"
            );
        }
    }
    if case.damaged_math {
        assert!(
            document.diagnostics.iter().any(|finding| {
                finding.impact == DiagnosticImpact::SemanticCoverage
                    && finding
                        .code
                        .as_deref()
                        .is_some_and(|code| code.starts_with("manual.equation"))
            }),
            "native collision retains accepted text and declares mathematical coverage"
        );
    }
    assert_eq!(text_body(content), case.body, "public text output");
    for options in [MarkdownOptions::default(), MarkdownOptions::READABLE] {
        let markdown = render_markdown_with_options(content, options);
        assert_eq!(
            markdown_body(&markdown, None),
            case.body,
            "CommonMark visible body: {markdown:?}"
        );
    }
    assert_search(content, &case.body);
}

#[test]
fn finite_equation_word_matrix_keeps_exact_rows_blanks_owners_and_consumers() {
    // The fixture removes only the reference's fixed five-cell page margin and
    // its filled first-row automatic blank. Per-row author whitespace survives.
    // Genuine \z collisions retain separate raw rows, with final accepted
    // cells chosen as the readable projection. Glyph overwrites and consumed
    // internal math blanks may use safe Text fallback instead of publishing an
    // expression that would reconstruct rejected native cells.
    super::case_failures::verify_cases(
        cases(),
        |case| case.id(),
        |case| {
            let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            assert_case(&original, case);
            let transported = super::json_roundtrip::json_string_roundtrip(&original);
            assert_eq!(transported, original);
            assert_case(&transported, case);
        },
    );
}

#[path = "equation_execution/families.rs"]
mod families;
