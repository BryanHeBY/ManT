//! Native manual links retain source ownership while their labels join prose.

use mant_codec::encode::{
    MarkdownNode, MarkdownOptions, render_addressable_markdown_with_options,
    render_markdown_with_options,
};
use mant_ir::{Block, DefinitionItem, EntryOwner, Inline, LinkTarget, ResolvedContent};
use mant_protocol::{QueryBundle, SearchCase, SearchQuery, SearchScope, SearchSyntax};

const HEADER: &str =
    ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn source(head: u8, body: u8, head_manual: bool, body_manual: bool, word: &str) -> String {
    let carriers = ["No", "Sy", "Em", "Li"];
    let fonts = ["", "\\fB", "\\fI", "\\f[CW]"];
    let first = if head_manual {
        format!(".Xr \"{}HEADX\" 1", fonts[usize::from(head)])
    } else {
        format!(".{} HEADX", carriers[usize::from(head)])
    };
    let second = if body_manual {
        format!(".Xr \"{}{word}\" 1", fonts[usize::from(body)])
    } else {
        format!(".{} {word}", carriers[usize::from(body)])
    };
    format!("{HEADER}.Bl -hang -width 2n\n.It Xo\n.sp\n{first}\n.Xc\n{second}\n.El\n")
}

fn item(content: &ResolvedContent) -> &DefinitionItem {
    content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap()
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(&items[0]),
            _ => None,
        })
        .unwrap()
}

fn paragraph(content: &ResolvedContent) -> &[Inline] {
    let blocks = &content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap()
        .blocks;
    let Block::List { items, .. } = &blocks[0] else {
        panic!("portable item {blocks:?}");
    };
    assert_eq!(items.len(), 1);
    items[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. }
                if mant_ir::inline_plain_text(children).contains("HEADX") =>
            {
                Some(children.as_slice())
            }
            _ => None,
        })
        .unwrap()
}

fn cells(nodes: &[Inline], style: u8, literal_destinations: bool, output: &mut Vec<(char, u8)>) {
    for node in nodes {
        match node {
            Inline::Text { value } | Inline::Code { value } => {
                let style = style | (u8::from(matches!(node, Inline::Code { .. })) * 4);
                output.extend(value.chars().map(|glyph| (glyph, style)));
            }
            Inline::Strong { children } => cells(children, style | 1, literal_destinations, output),
            Inline::Emphasis { children } => {
                cells(children, style | 2, literal_destinations, output);
            }
            Inline::Link { children, .. } => cells(children, style, literal_destinations, output),
            Inline::LineBreak { .. } => output.push(('\n', 0)),
            Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } if literal_destinations => {
                for target in std::iter::once(id.as_str())
                    .chain(fragment_aliases.iter().map(mant_ir::FragmentAlias::as_str))
                {
                    // All native-generated fixture destinations are safe
                    // ASCII ids; the reader retains this exact source text.
                    let marker = format!("<a id=\"{target}\"></a>");
                    output.extend(marker.chars().map(|glyph| (glyph, style)));
                }
            }
            Inline::Anchor { .. } => {}
            other @ Inline::Equation { .. } => panic!("unexpected phrase {other:?}"),
        }
    }
}

fn assert_label(nodes: &[Inline], head: u8, body: u8, label: &str, word: &str) {
    let mut actual = Vec::new();
    cells(nodes, 0, false, &mut actual);
    // Every exact pristine source has one leading row from HEAD's .sp.
    // No arbitrary prefix, extra delimiter or generated marker is ignored.
    let expected = std::iter::once(('\n', 0))
        .chain(
            [(label, head), (word, body)]
                .into_iter()
                .flat_map(|(word, style)| {
                    word.chars()
                        .map(move |glyph| (glyph, [0, 1, 2, 4][usize::from(style)]))
                }),
        )
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

fn assert_source_styles(content: &ResolvedContent, head: u8, body: u8, label: &str, word: &str) {
    let definition = item(content);
    let mut actual = Vec::new();
    cells(&definition.terms[0], 0, false, &mut actual);
    let Block::Paragraph { children, .. } = &definition.description[0] else {
        panic!("prose BODY");
    };
    cells(children, 0, false, &mut actual);
    let expected = std::iter::once(('\n', 0))
        .chain(
            [(label, head), (word, body)]
                .into_iter()
                .flat_map(|(word, style)| {
                    word.chars()
                        .map(move |glyph| (glyph, [0, 1, 2, 4][usize::from(style)]))
                }),
        )
        .collect::<Vec<_>>();
    assert_eq!(
        actual, expected,
        "source and real JSON keep every original native font cell"
    );
}

fn portable_head_style(
    content: &ResolvedContent,
    head: u8,
    body: u8,
    manual: bool,
    label: &str,
    anchors: bool,
) -> u8 {
    if !manual || body != 0 || !matches!(head, 1 | 2) {
        return head;
    }
    if anchors {
        let definition = item(content);
        let mut emitted = Vec::new();
        cells(&definition.terms[0], 0, true, &mut emitted);
        let Block::Paragraph { children, .. } = &definition.description[0] else {
            panic!("prose BODY");
        };
        cells(children, 0, true, &mut emitted);
        let text = emitted.iter().map(|(glyph, _)| *glyph).collect::<String>();
        let end = text.find(label).unwrap() + label.len();
        // Only a real retained destination between this label and BODY is
        // a syntax barrier. The current fixtures' ids contain no author words.
        if text[end..].starts_with("<a id=\"") {
            return head;
        }
    }
    // ')' followed directly by a plain alphanumeric word cannot close either
    // CommonMark style delimiter. Preserve glyphs with the existing plain
    // fallback; do not change native IR or invent spacing, HTML or sentinels.
    0
}

fn assert_literal_destinations(
    native: &ResolvedContent,
    imported: &ResolvedContent,
    label: &str,
    flatten_head: bool,
) -> String {
    let definition = item(native);
    let mut expected = Vec::new();
    cells(&definition.terms[0], 0, true, &mut expected);
    let Block::Paragraph { children, .. } = &definition.description[0] else {
        panic!("prose BODY");
    };
    cells(children, 0, true, &mut expected);
    if flatten_head {
        let text = expected.iter().map(|(glyph, _)| *glyph).collect::<String>();
        let start = text[..text.find(label).unwrap()].chars().count();
        for (_, style) in &mut expected[start..start + label.chars().count()] {
            *style = 0;
        }
    }
    let mut actual = Vec::new();
    cells(paragraph(imported), 0, false, &mut actual);
    assert_eq!(
        actual, expected,
        "only actual native destinations become literal HTML; author glyph/style cells are already independently checked in default mode"
    );
    expected.iter().map(|(glyph, _)| *glyph).collect()
}

fn assert_ast_owners(source: &str, word: &str) {
    use libmandoc_rs::{Node, NodeKind, Parser};
    fn find<'a>(node: &'a Node, predicate: &impl Fn(&Node) -> bool) -> Option<&'a Node> {
        if predicate(node) {
            Some(node)
        } else {
            node.children.iter().find_map(|node| find(node, predicate))
        }
    }
    let parsed = Parser::default()
        .parse_bytes("link-seam.1", source.as_bytes())
        .unwrap();
    let item = find(&parsed.document.root, &|node| {
        node.kind == NodeKind::Block && node.macro_name.as_deref() == Some("It")
    })
    .unwrap();
    for (kind, marker) in [(NodeKind::Head, "HEADX"), (NodeKind::Body, word)] {
        let part = item.children.iter().find(|node| node.kind == kind).unwrap();
        assert!(
            find(part, &|node| node
                .text
                .as_ref()
                .is_some_and(|text| text.contains(marker)))
            .is_some()
        );
    }
    let head = item
        .children
        .iter()
        .find(|node| node.kind == NodeKind::Head)
        .unwrap();
    assert!(
        find(head, &|node| node
            .text
            .as_ref()
            .is_some_and(|text| text.contains(word)))
        .is_none()
    );
}

fn assert_name_roots(owner: EntryOwner<'_>, body_word: &str) {
    let Some(facts) = owner.facts() else {
        return;
    };
    assert!(!facts.names.iter().any(|name| name.contains(body_word)));
    // A Manual-reference-only HEAD can have empty declaration names even
    // when its structural EntryFacts exist. Do not invent an eligible name.
    assert_eq!(facts.name_bindings.is_empty(), facts.names.is_empty());
    for binding in &facts.name_bindings {
        assert_ne!(binding.occurrences, []);
        for occurrence in &binding.occurrences {
            assert_eq!(
                mant_ir::inline_plain_text(&owner.form(occurrence).unwrap()),
                facts.names[binding.name]
            );
            assert!(
                occurrence
                    .parts
                    .iter()
                    .all(|part| matches!(part.root, mant_ir::EntryInlineRoot::Term { .. }))
            );
        }
    }
}

fn assert_manual_identity(nodes: &[Inline], manual: bool, word: &str) {
    fn links(nodes: &[Inline], output: &mut Vec<(LinkTarget, String)>) {
        for node in nodes {
            match node {
                Inline::Link {
                    target, children, ..
                } => {
                    output.push((target.clone(), mant_ir::inline_plain_text(children)));
                    links(children, output);
                }
                Inline::Strong { children } | Inline::Emphasis { children } => {
                    links(children, output);
                }
                _ => {}
            }
        }
    }
    let mut actual = Vec::new();
    links(nodes, &mut actual);
    let expected = if manual {
        vec![(
            LinkTarget::Manual {
                name: word.into(),
                manual_section: Some("1".into()),
            },
            format!("{word}(1)"),
        )]
    } else {
        vec![]
    };
    assert_eq!(
        actual, expected,
        "Link targets and complete children remain in their original root"
    );
}

fn assert_owned_artifact(
    content: &ResolvedContent,
    head_manual: bool,
    body_manual: bool,
    word: &str,
) {
    let definition = item(content);
    assert!(
        definition
            .layout
            .head_body_relation
            .joins_without_separator()
    );
    assert_eq!(definition.terms.len(), 1);
    let head = if head_manual { "HEADX(1)" } else { "HEADX" };
    assert_eq!(
        mant_ir::inline_plain_text(&definition.terms[0]),
        format!("\n{head}")
    );
    let owner = EntryOwner::Definition(definition);
    if !head_manual {
        assert_eq!(owner.facts().unwrap().names, ["HEADX"]);
    }
    assert_name_roots(owner, word);
    assert_manual_identity(&definition.terms[0], head_manual, "HEADX");
    let Block::Paragraph { children, .. } = &definition.description[0] else {
        panic!("prose BODY");
    };
    assert_manual_identity(children, body_manual, word);
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    let mapped=artifact.nodes().iter().filter(|mapped|matches!(mapped.node(),MarkdownNode::DocumentEntry {owner:EntryOwner::Definition(value),..} if std::ptr::eq(*value,definition))).collect::<Vec<_>>();
    assert_eq!(mapped.len(), usize::from(owner.facts().is_some()));
    for mapped in mapped {
        let bytes = &artifact.text()[mapped.range()];
        assert!(bytes.contains("HEADX") && bytes.contains(word));
    }
}

fn assert_queries(content: &ResolvedContent, head: &str, label: &str, joined_visible: bool) {
    let expected = format!("{head}{label}");
    let bad_code = format!("{head}``");
    let bad_strong = format!("{head}****");
    let bad_emphasis = format!("{head}**");
    let artifact = render_addressable_markdown_with_options(content, MarkdownOptions::ADDRESSABLE);
    for (word, found) in [
        (expected.as_str(), joined_visible),
        (head, true),
        (label, true),
        (bad_code.as_str(), false),
        (bad_strong.as_str(), false),
        (bad_emphasis.as_str(), false),
    ] {
        let response = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: word.into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope: SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 100,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(response.total, u32::from(found), "{word}");
        for hit in response.matches {
            assert_ne!(hit.occurrences, []);
            for occurrence in hit.occurrences {
                assert_eq!(occurrence.matched_text, word);
                let range = usize::try_from(occurrence.markdown.start_byte).unwrap()
                    ..usize::try_from(occurrence.markdown.end_byte).unwrap();
                let bytes = &artifact.text()[range];
                if word == expected {
                    assert!(bytes.contains("HEADX") && bytes.contains(label));
                } else {
                    assert_eq!(bytes, word);
                }
            }
        }
    }
}

fn check_source(
    source: &str,
    head: u8,
    body: u8,
    head_manual: bool,
    body_manual: bool,
    word: &str,
) {
    assert_ast_owners(source, word);
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = serde_json::to_string(&QueryBundle::from(&original)).unwrap();
    let decoded: ResolvedContent = serde_json::from_str::<QueryBundle>(&wire).unwrap().into();
    assert_eq!(original, decoded);
    let head_label = if head_manual { "HEADX(1)" } else { "HEADX" };
    let label = if body_manual {
        format!("{word}(1)")
    } else {
        word.into()
    };
    // print_mdoc_node() restores a stack index, not an old font value
    // (mdoc_term.c:333/409; term.c:512/538). Xr calls term_word() directly;
    // a font escape replaces the current slot, so an unqualified second Xr
    // inherits it. No/Li use a scoped Roman push and therefore do not.
    // The exact pristine overstrike rows establish Bold/Em inheritance;
    // the selected source-semantic CW enhancement retains the Code state.
    let body = if head_manual && body_manual && body == 0 {
        head
    } else {
        body
    };
    for content in [&original, &decoded] {
        assert_owned_artifact(content, head_manual, body_manual, word);
        assert_source_styles(content, head, body, head_label, &label);
        let text = mant_render::render_query_man(content);
        let line = text.lines().find(|line| line.contains("HEADX")).unwrap();
        assert_eq!(line.trim_start_matches(' '), format!("{head_label}{label}"));
        assert_queries(content, head_label, &label, true);
        for preserve_anchors in [false, true] {
            let markdown = render_markdown_with_options(
                content,
                MarkdownOptions {
                    preserve_anchors,
                    preserve_semantics: false,
                },
            );
            let imported = mant_loader::load_markdown_text(&markdown, None).unwrap();
            let projected_head = portable_head_style(
                content,
                head,
                body,
                head_manual,
                head_label,
                preserve_anchors,
            );
            if preserve_anchors {
                let literal = assert_literal_destinations(
                    content,
                    &imported,
                    head_label,
                    projected_head != head,
                );
                assert_queries(
                    &imported,
                    head_label,
                    &label,
                    literal.contains(&format!("{head_label}{label}")),
                );
            } else {
                assert_label(
                    paragraph(&imported),
                    projected_head,
                    body,
                    head_label,
                    &label,
                );
                assert_queries(&imported, head_label, &label, true);
            }
        }
    }
    assert_eq!(
        original, decoded,
        "presentation cannot mutate Link or name roots"
    );
}

#[test]
fn native_joined_manual_labels_keep_styles_names_and_actual_query_ranges() {
    // The 32 exact sources ran pristine ASCII/UTF-8/HTML/tree/lint before
    // these assertions. termp_xr_pre() executes its label and generated (1)
    // in the shared word stream (mdoc_term.c:1155-1177); HEAD post leaves an
    // occupied HANG row beyond its 4-cell BODY origin (term.c:113-253).
    // The 16 Manual cases have only the retained referenced-manual-not-found
    // STYLE lint finding; none is misclassified as a clean lint0 sample.
    for head in 0..4 {
        for body in 0..4 {
            for manual in [false, true] {
                check_source(
                    &source(head, body, false, manual, "BodyWord"),
                    head,
                    body,
                    false,
                    manual,
                    "BodyWord",
                );
            }
        }
    }
}

#[test]
fn reported_code_manual_link_cannot_leak_backticks_into_visible_search() {
    // Exact reported source ran the five pristine profiles first, including
    // its referenced-manual-not-found STYLE status. Native visible label is
    // HEADXBODY(1), never HEADX``BODY(1); JSON retains the Manual Link.
    check_source(
        &source(3, 3, false, true, "BODY"),
        3,
        3,
        false,
        true,
        "BODY",
    );
}

#[test]
fn actual_native_head_and_both_manual_links_share_joined_context_without_owner_mutation() {
    // Every one of these 32 exact sources ran the five pristine profiles first.
    // termp_xr_pre() constructs HEADX(1) in HEAD and optionally BodyWord(1)
    // in BODY; each final occupied HEAD row is beyond the real 4-cell origin.
    // Referenced-manual STYLE findings remain recorded, never clean lint0.
    for head in 0..4 {
        for body in 0..4 {
            for body_manual in [false, true] {
                check_source(
                    &source(head, body, true, body_manual, "BodyWord"),
                    head,
                    body,
                    true,
                    body_manual,
                    "BodyWord",
                );
            }
        }
    }
}
