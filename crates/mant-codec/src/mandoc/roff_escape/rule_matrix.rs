//! Pristine-reference grammar branches with real owned AST → IR → JSON.
//!
//! Gold is recorded before assertions by `record_escape_rule_matrix.py`.
//! CVS `roff_escape.c` classifies standard IGNORE arguments as known syntax,
//! advances the fixed size quote before scanning, and matches escaped
//! delimiters on trigger identity rather than displayed glyph.

use mant_ir::{
    Block, Inline,
    visit::{Visit, walk_block},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    count: usize,
    unique_sources: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    source: String,
    rule_id: String,
    carrier: String,
    expected_rows: Vec<String>,
    metadata: serde_json::Value,
    expected_after_styles: Option<Vec<u8>>,
    style_policy: Option<FontStylePolicy>,
}

#[derive(Deserialize)]
struct FontStylePolicy {
    expected_product_after_styles: Vec<u8>,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/escape_rules.json")).unwrap()
}

fn native_inline(nodes: &[Inline]) -> String {
    let mut text = String::new();
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                text.push_str(value);
            }
            Inline::Strong { children }
            | Inline::PortableDisplay { children, .. }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => text.push_str(&native_inline(children)),
            Inline::LineBreak { .. } => text.push('\n'),
            Inline::Anchor { .. } => {}
        }
    }
    text
}

fn word_rows(rows: Vec<String>) -> Vec<String> {
    // These grammar cases contain no authored indentation or fixed blanks.
    // Preserve row topology and ordinary separator existence; numeric device
    // padding is not an assertion about responsive terminal column origins.
    rows.into_iter()
        .map(|row| {
            row.split(' ')
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[derive(Default)]
struct Rows(Vec<String>);

impl<'ir> Visit<'ir> for Rows {
    fn visit_block(&mut self, block: &'ir Block) {
        match block {
            Block::Paragraph {
                children, layout, ..
            }
            | Block::Preformatted {
                children, layout, ..
            } => {
                self.0.extend(std::iter::repeat_n(
                    String::new(),
                    usize::from(layout.spacing_before_lines),
                ));
                self.0
                    .extend(native_inline(children).split('\n').map(str::to_owned));
            }
            Block::VerticalSpace { lines, .. } => self
                .0
                .extend(std::iter::repeat_n(String::new(), usize::from(*lines))),
            Block::DefinitionList { items, .. } => {
                for item in items {
                    let term = item
                        .terms
                        .iter()
                        .map(|term| native_inline(term))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let mut body = Rows::default();
                    for block in &item.description {
                        body.visit_block(block);
                    }
                    if item.inline_description().is_some() && !body.0.is_empty() {
                        let gap = " ".repeat(usize::from(item.layout.min_term_gap_columns));
                        self.0.push(term + &gap + &body.0.remove(0));
                    } else if !term.is_empty() {
                        self.0.extend(term.split('\n').map(str::to_owned));
                    }
                    self.0.extend(body.0);
                }
            }
            _ => walk_block(self, block),
        }
    }
}

#[test]
fn known_and_size_syntax_share_native_extent_in_owned_json() {
    let fixture = fixture();
    assert_eq!((fixture.count, fixture.unique_sources), (2428, 2271));
    let mut failures = Vec::new();
    let mut count = 0;
    for case in fixture
        .cases
        .iter()
        .filter(|case| matches!(case.rule_id.as_str(), "E01" | "E02" | "RC01" | "RC02"))
    {
        let document = crate::mandoc::parse_plain_manual(
            std::path::Path::new("escape-rule.1"),
            case.source.as_bytes(),
        )
        .unwrap();
        let json = serde_json::to_string(&document).unwrap();
        let decoded: mant_ir::Document = serde_json::from_str(&json).unwrap();
        let section = decoded
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "DESCRIPTION")
            .unwrap();
        let mut actual = Rows::default();
        for block in &section.blocks {
            actual.visit_block(block);
        }
        let actual = word_rows(actual.0);
        let expected = word_rows(case.expected_rows.clone());
        if actual != expected {
            failures.push(format!(
                "{}\n  native={expected:?}\n  actual={actual:?}",
                case.name
            ));
        }
        count += 1;
    }
    assert_eq!(count, 1764);
    assert!(
        failures.is_empty(),
        "{} grammar differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn nested_delimiters_and_output_carriers_keep_native_rows() {
    let mut failures = Vec::new();
    let mut count = 0;
    for case in fixture()
        .cases
        .iter()
        .filter(|case| matches!(case.rule_id.as_str(), "E03" | "E04" | "E05" | "E06" | "E07"))
    {
        // ManT's documented N recovery preserves unsupported source spelling,
        // unlike native zero-width ERROR output. These exact cases are still
        // frozen, replayed, and separately qualified by the acceptance ledger.
        if case.rule_id == "E03"
            && case.metadata["outer"] == "N"
            && case.metadata["completion"] != "closed"
        {
            continue;
        }
        // Pristine has no project safety cap. S4's completeness tests assert
        // protected output + ContentCoverage rather than pristine suffixes.
        if case.rule_id == "E07"
            && case.metadata["depth"]
                .as_u64()
                .is_some_and(|depth| depth > 256)
        {
            continue;
        }
        let document = crate::mandoc::parse_plain_manual(
            std::path::Path::new("escape-carrier.1"),
            case.source.as_bytes(),
        )
        .unwrap();
        let json = serde_json::to_string(&document).unwrap();
        let decoded: mant_ir::Document = serde_json::from_str(&json).unwrap();
        let section = decoded
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "DESCRIPTION")
            .unwrap();
        let mut actual = Rows::default();
        for block in &section.blocks {
            actual.visit_block(block);
        }
        let actual = word_rows(actual.0);
        let expected = word_rows(case.expected_rows.clone());
        if actual != expected {
            failures.push(format!(
                "{}\n  native={expected:?}\n  actual={actual:?}",
                case.name
            ));
        }
        count += 1;
    }
    assert_eq!(count, 636);
    assert!(
        failures.is_empty(),
        "{} carrier differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn delimiter_and_counted_completion_are_separate_from_consumed_extent() {
    use super::scanner::{ArgumentCompletion, scan_argument};
    // Exact source variants are in the pristine E01/E02 fixture above.
    for (source, trigger, payload, end, completion) in [
        ("'12'Y", 's', "12", 4, ArgumentCompletion::Complete),
        ("+'12'Y", 's', "12", 5, ArgumentCompletion::Complete),
        ("'1\\&2'Y", 's', "1\\&2", 6, ArgumentCompletion::Complete),
        ("(12Y", 's', "12", 3, ArgumentCompletion::Complete),
        ("[12]Y", 's', "12", 4, ArgumentCompletion::Complete),
        ("'12", 's', "12", 3, ArgumentCompletion::Incomplete),
        ("[]Y", 'm', "", 2, ArgumentCompletion::Complete),
        ("[BI]Y", 'f', "BI", 4, ArgumentCompletion::Complete),
    ] {
        let chars: Vec<_> = source.chars().collect();
        let result = scan_argument(&chars, 0, trigger);
        assert_eq!(result.end, end, "{source}");
        assert_eq!(result.completion, completion, "{source}");
        assert_eq!(
            result
                .payload
                .map(|range| chars[range].iter().collect::<String>())
                .as_deref(),
            Some(payload),
            "{source}"
        );
    }
    // Reading case metadata is itself a guard against silently losing axes.
    assert!(
        fixture()
            .cases
            .iter()
            .any(|case| case.carrier == "Lk" && case.metadata["position"] == "internal")
    );
}

#[test]
fn nested_consumption_and_failed_prefixes_have_bounded_scan_work() {
    use super::scanner::take_scan_work;
    // The exact pristine E07 sources/profile hashes precede this assertion.
    // Work units count task dispatch plus index movement, including rejected
    // and rewind paths; no product path clones/rescans accumulated output.
    for case in fixture().cases.iter().filter(|case| case.rule_id == "E07") {
        let word = case
            .source
            .lines()
            .find_map(|line| {
                line.strip_prefix(".No \"")
                    .and_then(|line| line.strip_suffix('"'))
            })
            .unwrap();
        let _ = take_scan_work();
        let result = super::decode_with_status(word);
        let (scan_units, depth) = take_scan_work();
        assert!(
            scan_units <= word.chars().count().saturating_mul(12) + 16,
            "{}: {scan_units}",
            case.name
        );
        assert!(depth <= 257, "{}: {depth}", case.name);
        if case.metadata["depth"]
            .as_u64()
            .is_some_and(|depth| depth > 256)
        {
            assert!(result.budget_exhausted, "{}", case.name);
        }
    }
}

#[test]
fn malformed_bracketed_shapes_share_native_consumed_extent() {
    use super::scanner::{ArgumentCompletion, scan_argument};
    // Both exact malformed-name sources were rendered by pristine before
    // this assertion: ESC_ARG consumes the initial blank, then leaves the
    // name-like suffix as document text. SPECIAL may retain source recovery;
    // it must nevertheless use the same consumed extent as IGNORE.
    let standard = scan_argument(&"[ name]Z".chars().collect::<Vec<_>>(), 0, 'm');
    let named = scan_argument(&" name]Z".chars().collect::<Vec<_>>(), 0, '[');
    assert_eq!(standard.completion, ArgumentCompletion::Rejected);
    assert_eq!(named.completion, ArgumentCompletion::Rejected);
    assert_eq!(standard.end, named.end + 1);
    assert!(standard.payload.is_none() && named.payload.is_none());
    assert_eq!(super::visible_text(r"A\m[ name]Z"), "Aname]Z");
    // Source-spelling recovery for SPECIAL remains visible. The suffix
    // belongs to ordinary text; it is not swallowed with a rejected name.
    assert_eq!(super::visible_text(r"A\[ name]Z"), r"A\[ name]Z");
}

fn styled_characters(nodes: &[Inline], style: u8, output: &mut Vec<(char, u8)>) {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                output.extend(value.chars().map(|character| (character, style)));
            }
            Inline::Strong { children } => styled_characters(children, style | 1, output),
            Inline::Emphasis { children } => styled_characters(children, style | 2, output),
            Inline::PortableDisplay { children, .. } | Inline::Link { children, .. } => {
                styled_characters(children, style, output);
            }
            Inline::LineBreak { .. } => output.push(('\n', 0)),
            Inline::Equation { .. } | Inline::Anchor { .. } => {}
        }
    }
}

#[test]
fn font_postclassification_preserves_both_registers_and_native_style() {
    // roff_escape.c only invokes mandoc_font after a complete FONT operand;
    // term.c ignores ERROR entirely. Complete empty operands select previous.
    // Expected masks are observed pristine backspace overlays, never inferred
    // from candidate output or the spelling of an invalid font name.
    // The exact C/V/VB/VI cases retain the already documented Pandoc font
    // extension (patch 0013); both original native masks and policy are frozen.
    let mut count = 0;
    for case in fixture()
        .cases
        .iter()
        .filter(|case| case.metadata["branch"] == "font-postclass")
    {
        let document = crate::mandoc::parse_plain_manual(
            std::path::Path::new("font-postclass.1"),
            case.source.as_bytes(),
        )
        .unwrap();
        let json = serde_json::to_string(&document).unwrap();
        let decoded: mant_ir::Document = serde_json::from_str(&json).unwrap();
        let section = decoded
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "DESCRIPTION")
            .unwrap();
        let mut characters = Vec::new();
        for block in &section.blocks {
            if let Block::Paragraph { children, .. } | Block::Preformatted { children, .. } = block
            {
                styled_characters(children, 0, &mut characters);
            }
        }
        let starts: Vec<_> = characters
            .windows(5)
            .enumerate()
            .filter_map(|(index, window)| {
                window
                    .iter()
                    .map(|cell| cell.0)
                    .eq("AFTER".chars())
                    .then_some(index)
            })
            .collect();
        assert_eq!(starts.len(), 1, "{}", case.name);
        let actual: Vec<_> = characters[starts[0]..starts[0] + 5]
            .iter()
            .map(|cell| cell.1)
            .collect();
        let expected = case.style_policy.as_ref().map_or_else(
            || case.expected_after_styles.as_ref().unwrap(),
            |policy| &policy.expected_product_after_styles,
        );
        assert_eq!(&actual, expected, "{}", case.name);
        count += 1;
    }
    assert_eq!(count, 81);
}

#[test]
fn mandatory_eof_and_font_result_keep_independent_completion_and_extent() {
    use super::grammar::EscapeKind;
    use super::scanner::{ArgumentCompletion, scan_argument};
    // All exact operand spellings are recorded in the append-only E cohort.
    // roff_escape.c:320 pre-scan EOF keeps IGNORE; the standard shape switch
    // runs later and leaves an empty opener outside iend. mandoc_font runs
    // only after a complete operand, and invalid complete names are ERROR.
    for (operand, trigger, end, completion, kind) in [
        ("", 's', 0, ArgumentCompletion::Missing, EscapeKind::Ignore),
        ("+", 's', 1, ArgumentCompletion::Missing, EscapeKind::Ignore),
        ("[", 's', 1, ArgumentCompletion::Missing, EscapeKind::Ignore),
        (
            "[",
            'm',
            0,
            ArgumentCompletion::Incomplete,
            EscapeKind::Error,
        ),
        (
            "(",
            'f',
            0,
            ArgumentCompletion::Incomplete,
            EscapeKind::Error,
        ),
        (
            "[BI",
            'f',
            3,
            ArgumentCompletion::Incomplete,
            EscapeKind::Error,
        ),
        (
            "[garbage]",
            'f',
            9,
            ArgumentCompletion::Complete,
            EscapeKind::Error,
        ),
        ("[]", 'f', 2, ArgumentCompletion::Complete, EscapeKind::Font),
    ] {
        let result = scan_argument(&operand.chars().collect::<Vec<_>>(), 0, trigger);
        assert_eq!(
            (result.end, result.completion, result.kind),
            (end, completion, kind),
            "{trigger}{operand}"
        );
    }
}
