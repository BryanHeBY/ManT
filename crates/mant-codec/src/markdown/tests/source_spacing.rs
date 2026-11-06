//! Physical gaps are independent of a container's parser range indentation.

use mant_ir::{Block, Inline, visit::Visit};
use std::fmt::Write as _;

use super::parse_markdown;

#[derive(Default)]
struct Paragraphs<'a>(Vec<&'a Block>);

#[derive(Default)]
struct Receivers {
    code: usize,
    rules: usize,
}

impl Visit<'_> for Receivers {
    fn visit_block(&mut self, block: &Block) {
        self.code += usize::from(matches!(block, Block::Preformatted { .. }));
        self.rules += usize::from(matches!(block, Block::ThematicBreak { .. }));
        mant_ir::visit::walk_block(self, block);
    }
}

impl<'a> Visit<'a> for Paragraphs<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        if matches!(block, Block::Paragraph { .. }) {
            self.0.push(block);
        }
        mant_ir::visit::walk_block(self, block);
    }
}

fn wrap(mut body: String, depth: usize) -> String {
    for _ in 0..depth {
        let mut wrapped = String::from("- OUTER\n\n");
        for line in body.lines() {
            writeln!(wrapped, "  {line}").unwrap();
        }
        body = wrapped;
    }
    body
}

fn assert_after(source: &str, spacing: u16) {
    let parsed = parse_markdown(source, None).unwrap();
    let mut paragraphs = Paragraphs::default();
    paragraphs.visit_document(&parsed.document);
    let found = paragraphs
        .0
        .into_iter()
        .filter(|block| {
            matches!(block, Block::Paragraph { children, .. }
            if mant_ir::inline_plain_text(children) == "AFTER")
        })
        .collect::<Vec<_>>();
    let [
        Block::Paragraph {
            layout,
            source: Some(span),
            ..
        },
    ] = found.as_slice()
    else {
        panic!("one actual AFTER owner: {source:?}");
    };
    assert_eq!(layout.spacing_before_lines, spacing, "{source:?}");
    let range = span.byte_range.unwrap();
    assert_eq!(
        usize::try_from(range.start.get()).unwrap(),
        source.find("AFTER").unwrap()
    );
    assert_eq!(usize::try_from(range.end.get()).unwrap(), source.len());
}

#[test]
fn nested_list_ranges_keep_absorbed_blank_lines_before_the_next_paragraph() {
    // Locked pulldown-cmark firstpass::finish_list/pop ends a container at
    // the current cursor, after outer indentation. Its range can therefore
    // contain "LAST\n\n  " even though the next paragraph starts at AFTER.
    for depth in 0..=3 {
        for marker in ["-", "1."] {
            for prefix in ["", "<!-- boundary -->\n\n", "[](https://example.test)\n\n"] {
                for ending in ["\n", "\r\n", "\r"] {
                    let body = format!("{marker} 界😀 LAST\n\n{prefix}AFTER\n");
                    let source = wrap(body, depth).replace('\n', ending);
                    assert_after(&source, 1);
                }
            }
        }
    }
}

#[test]
fn a_literal_internal_empty_row_is_not_spacing_after_its_closing_fence() {
    for depth in 0..=3 {
        for ending in ["\n", "\r\n", "\r"] {
            for gap in ["", "\n", " \t\n"] {
                let body = format!("1. LAST\n\n```\nCODE\n\n```\n{gap}AFTER\n");
                let source = wrap(body, depth).replace('\n', ending);
                assert_after(&source, u16::from(!gap.is_empty()));
            }
        }
    }
}

#[test]
fn adjacent_fences_and_rules_do_not_acquire_an_unrequested_gap() {
    for depth in 0..=3 {
        for ending in ["\n", "\r\n", "\r"] {
            for receiver in ["```\nCODE\n```\n", "***\n"] {
                for gap in ["", "\n"] {
                    let source = wrap(format!("1. LAST\n{gap}{receiver}AFTER\n"), depth)
                        .replace('\n', ending);
                    let parsed = parse_markdown(&source, None).unwrap();
                    let mut receivers = Receivers::default();
                    receivers.visit_document(&parsed.document);
                    let expected = if receiver.starts_with("```") {
                        (1, 0)
                    } else {
                        (0, 1)
                    };
                    assert_eq!((receivers.code, receivers.rules), expected, "{source:?}");
                    let mut paragraphs = Paragraphs::default();
                    paragraphs.visit_document(&parsed.document);
                    assert!(paragraphs.0.iter().any(|block| {
                        matches!(block, Block::Paragraph { children, .. }
                            if mant_ir::inline_plain_text(children) == "AFTER")
                    }));
                    assert_after(&source, 0);
                }
            }
        }
    }
}

#[test]
fn literal_and_zero_width_navigation_content_keep_their_independent_owners() {
    let source = "- OUTER\n\n  1. LAST\n\n  [](https://example.test)\n\n  AFTER\n";
    let parsed = parse_markdown(source, None).unwrap();
    let mut paragraphs = Paragraphs::default();
    paragraphs.visit_document(&parsed.document);
    let labels = paragraphs
        .0
        .iter()
        .filter_map(|block| {
            let Block::Paragraph { children, .. } = block else {
                return None;
            };
            children.iter().find_map(|inline| match inline {
                Inline::Link {
                    target, children, ..
                } => Some((target.to_uri(), children.len())),
                _ => None,
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(labels, [(Some("https://example.test".into()), 0)]);
    assert_after(source, 1);
}
