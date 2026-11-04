//! Original IR is the oracle; exported Markdown is only the actual input.

use super::*;

mod finalization;

#[derive(Debug, Eq, PartialEq)]
struct Scalar {
    character: char,
    styles: u8,
    link: Option<usize>,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Meaning {
    scalars: Vec<Scalar>,
    links: Vec<(mant_ir::LinkTarget, Option<String>)>,
}

impl Meaning {
    fn text(&mut self, value: &str, styles: u8, link: Option<usize>) {
        self.scalars.extend(value.chars().map(|character| {
            // Code rows split spans; hard-row scalars have neither style nor
            // link ownership in the established portable Markdown contract.
            let (styles, link) = if character == '\n' {
                (0, None)
            } else {
                (styles, link)
            };
            Scalar {
                character,
                styles,
                link,
            }
        }));
    }

    fn visit(&mut self, nodes: &[Inline], styles: u8, link: Option<usize>) {
        for node in nodes {
            match node {
                Inline::Text { value } => self.text(value, styles, link),
                Inline::Code { value } | Inline::Equation { value, .. } => {
                    self.text(value, styles | 4, link);
                }
                Inline::Strong { children } => self.visit(children, styles | 1, link),
                Inline::Emphasis { children } => self.visit(children, styles | 2, link),
                Inline::Link {
                    target,
                    title,
                    children,
                } => {
                    let owner = self.links.len();
                    self.links.push((target.clone(), title.clone()));
                    self.visit(children, styles, Some(owner));
                }
                Inline::LineBreak {} => self.text("\n", 0, None),
                Inline::Anchor { .. } => {}
            }
        }
    }
}

fn meaning(nodes: &[Inline]) -> Meaning {
    let mut result = Meaning::default();
    result.visit(nodes, 0, None);
    result
}

fn parsed_children(markdown: &str) -> Vec<Inline> {
    let parsed = crate::parse_markdown(markdown, None).unwrap();
    let [Block::Paragraph { children, .. }] = parsed.document.blocks.as_slice() else {
        panic!("one original phrasing owner: {markdown:?}");
    };
    children.clone()
}

fn assert_allowed_styles(expected: &Meaning, actual: &Meaning, markdown: &str) {
    assert_eq!(actual.links, expected.links, "{markdown}");
    assert_eq!(actual.scalars.len(), expected.scalars.len(), "{markdown}");
    for (expected, actual) in expected.scalars.iter().zip(&actual.scalars) {
        assert_eq!(actual.character, expected.character, "{markdown}");
        assert_eq!(actual.link, expected.link, "{markdown}");
        assert_eq!(actual.styles & 4, expected.styles & 4, "{markdown}");
        // Delimiter choices may omit an unrepresentable style; a rejected
        // whole sequence uses the existing all-unstyled re-encoding fallback.
        // Only Strong/Emphasis can be lost. No scalar may acquire a style
        // absent from the original IR, nor lose code or link identity.
        assert_eq!(actual.styles & !expected.styles, 0, "{markdown}");
    }
}

fn two_rounds(original: &[Inline]) -> Meaning {
    let expected = meaning(original);
    let first = inline::render_inline(original, MarkdownOptions::default());
    let first_nodes = parsed_children(&first);
    let first_meaning = meaning(&first_nodes);
    assert_allowed_styles(&expected, &first_meaning, &first);
    let second = inline::render_inline(&first_nodes, MarkdownOptions::default());
    let second_nodes = parsed_children(&second);
    assert_eq!(
        meaning(&second_nodes),
        first_meaning,
        "{first:?} -> {second:?}"
    );
    first_meaning
}

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn styled(strong: bool, children: Vec<Inline>) -> Inline {
    if strong {
        Inline::Strong { children }
    } else {
        Inline::Emphasis { children }
    }
}

fn link(children: Vec<Inline>, title: &str) -> Inline {
    Inline::Link {
        target: mant_ir::LinkTarget::External {
            uri: "https://example.test/中(a)%25".into(),
        },
        title: Some(title.into()),
        children,
    }
}

#[test]
fn nested_code_and_link_regressions_read_back_from_the_original_ir_twice() {
    // The outer Strong used to change the accepted child's underscore
    // pairing. Expected text/code/link/style comes from the original parse,
    // never from an already wrapped candidate or a slice of its output.
    for source in [r"**_`a`*b\**_**", r"**_[a](https://a.test)*b\**_**"] {
        let original = parsed_children(source);
        assert_eq!(
            meaning(&original)
                .scalars
                .iter()
                .map(|scalar| scalar.character)
                .collect::<String>(),
            "ab*"
        );
        assert_eq!(two_rounds(&original), meaning(&original));
    }
}

#[test]
fn flat_markdown_sources_keep_exact_styles_code_and_links_for_two_rounds() {
    for source in [
        r"**--arg=**_reuse_level_",
        r"_é中=_**参数\_值**",
        r#"***code `x*_y`*** [**label**](https://a.test "owner")"#,
        "**A**  \n_[B](https://b.test \"title\")_",
    ] {
        let original = parsed_children(source);
        assert_eq!(two_rounds(&original), meaning(&original), "{source}");
    }
}

#[test]
fn conservative_intraword_style_loss_has_an_explicit_projection() {
    // Existing conservative policy: delimiters cannot retain this isolated
    // bold hyphen without changing its contiguous spelling. The outer
    // emphasis, every scalar and both subsequent readbacks remain exact.
    let original = [styled(
        false,
        vec![text("x"), styled(true, vec![text("-")]), text("y")],
    )];
    let mut expected = meaning(&original);
    assert_eq!(expected.scalars[1].styles, 3);
    expected.scalars[1].styles = 2;
    assert_eq!(two_rounds(&original), expected);
}

#[test]
fn unicode_edge_whitespace_keeps_exact_scalars_and_the_representable_inner_style() {
    // pulldown-cmark firstpass::delim_run_can_open/close uses Unicode
    // is_whitespace, so raw edge NBSP/other Zs cannot be inside an emphasis
    // delimiter. Only those two outer style bits are unrepresentable: HEAD,
    // protected code padding, link identity and nested styles remain exact.
    for whitespace in ['\u{a0}', '\u{2003}', '\u{202f}', '\u{3000}'] {
        for strong in [false, true] {
            for adjacent in [false, true] {
                let edge = whitespace.to_string();
                let mut nodes = vec![styled(
                    strong,
                    vec![
                        text(&edge),
                        text("HEAD"),
                        Inline::Code {
                            value: format!(" {whitespace}a{whitespace} "),
                        },
                        link(vec![styled(!strong, vec![text("label")])], "original label"),
                        text(&edge),
                    ],
                )];
                if adjacent {
                    nodes.insert(0, text("LEFT"));
                    nodes.push(text("RIGHT"));
                }
                let mut expected = meaning(&nodes);
                let first = if adjacent { 4 } else { 0 };
                let last = expected.scalars.len() - 1 - if adjacent { 5 } else { 0 };
                let bit = if strong { 1 } else { 2 };
                assert_eq!(expected.scalars[first].character, whitespace);
                assert_eq!(expected.scalars[last].character, whitespace);
                expected.scalars[first].styles &= !bit;
                expected.scalars[last].styles &= !bit;
                assert_eq!(two_rounds(&nodes), expected);
            }
        }
    }
}

#[test]
fn nested_and_adjacent_source_ir_keeps_scalars_code_and_link_coverage() {
    // 144 bounded cases: outer/inner Strong or Emphasis, three leaf kinds,
    // three paired Unicode/marker spellings, base style depths 3/5 and
    // single/adjacent roots. Styled link labels add one level. Same-style
    // repetition and alternating nesting are both present. The independent
    // projection classifies permitted style loss separately from exact text,
    // code, link occurrences and hard rows.
    for outer in [false, true] {
        for inner in [false, true] {
            for leaf in 0..3 {
                for (value, tail) in [("a", "b*"), ("é中", "b_"), ("a*_b", "中—_*")] {
                    for extra in [0, 2] {
                        for adjacent in [false, true] {
                            let leaf = match leaf {
                                0 => text(value),
                                1 => Inline::Code {
                                    value: value.into(),
                                },
                                _ => link(vec![styled(!inner, vec![text(value)])], "label"),
                            };
                            let mut node =
                                styled(inner, vec![leaf, styled(inner, vec![text(tail)])]);
                            for level in 0..extra {
                                node = styled(level % 2 == 0, vec![node]);
                            }
                            let mut nodes = vec![styled(outer, vec![node])];
                            if adjacent {
                                nodes.insert(0, styled(!outer, vec![text("HEAD")]));
                                nodes.push(text("TAIL"));
                            }
                            two_rounds(&nodes);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn nested_labels_whitespace_and_hard_rows_keep_each_original_owner() {
    let nodes = [
        text(" \t"),
        styled(
            true,
            vec![styled(
                false,
                vec![
                    Inline::Code {
                        value: " a \n中_*".into(),
                    },
                    text(" * _ &amp; "),
                    link(vec![styled(true, vec![text("first_")])], "one owner"),
                    Inline::line_break(),
                    Inline::line_break(),
                    link(vec![styled(false, vec![text("second*")])], "second owner"),
                ],
            )],
        ),
        text("\u{a0} \t"),
        Inline::line_break(),
    ];
    two_rounds(&nodes);
    // Equal destinations/titles are still distinct occurrences and ranges.
    two_rounds(&[
        styled(false, vec![link(vec![text("A")], "same")]),
        styled(true, vec![link(vec![text("B")], "same")]),
    ]);
}
