//! Ordered name parts retain their actual phase, owner and row effects.

use libmandoc_rs::{MacroToken, MdocMacro, Node, NodeKind, Parser};
use mant_ir::{Block, Inline, ResolvedContent, inline_plain_text};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    reference_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    shape: Shape,
    font_control: bool,
    rows: Vec<String>,
    portable_rows: Vec<String>,
    font_cells: Vec<(char, u8)>,
    ast: AstFacts,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
struct Shape {
    head: Part,
    body: Part,
    prefix: Prefix,
    owner: Owner,
    fill: Fill,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
enum Part {
    Empty,
    Nonempty,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
enum Prefix {
    Absent,
    Present,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
enum Owner {
    Top,
    Head,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
enum Fill {
    Filled,
    Nofill,
}

#[derive(Deserialize)]
struct AstFacts {
    nm_source: (u32, u32),
    outer_source: Option<(u32, u32)>,
}

fn first(node: &Node, token: MdocMacro, kind: NodeKind) -> Option<&Node> {
    if node.macro_token == Some(MacroToken::Mdoc(token)) && node.kind == kind {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| first(child, token, kind))
    }
}

fn contains(node: &Node, id: u32) -> bool {
    node.id == id || node.children.iter().any(|child| contains(child, id))
}

fn assert_ast(case: &Case) {
    let parsed = Parser::default()
        .parse_bytes(&case.id, case.source.as_bytes())
        .unwrap();
    let nm = first(&parsed.document.root, MdocMacro::Nm, NodeKind::Block).unwrap();
    let hi = nm
        .children
        .iter()
        .position(|part| part.kind == NodeKind::Head)
        .unwrap();
    let head = &nm.children[hi];
    let body = nm
        .children
        .iter()
        .find(|part| part.kind == NodeKind::Body)
        .unwrap();
    assert_eq!(
        !head.children.is_empty(),
        case.shape.head == Part::Nonempty,
        "{}",
        case.id
    );
    assert_eq!(
        !body.children.is_empty(),
        case.shape.body == Part::Nonempty,
        "{}",
        case.id
    );
    assert_eq!(
        nm.flags.no_fill,
        case.shape.fill == Fill::Nofill,
        "{}",
        case.id
    );
    let prefix = nm.children[..hi]
        .iter()
        .filter_map(|part| part.text.as_deref())
        .collect::<String>();
    assert_eq!(
        prefix,
        if case.shape.prefix == Prefix::Present {
            "("
        } else {
            ""
        },
        "{}",
        case.id
    );
    assert_eq!((nm.line, nm.column), case.ast.nm_source, "{}", case.id);
    let outer = first(&parsed.document.root, MdocMacro::It, NodeKind::Block);
    let outer_head = outer.and_then(|node| {
        node.children
            .iter()
            .find(|part| part.kind == NodeKind::Head)
    });
    assert_eq!(
        outer_head.is_some_and(|node| contains(node, nm.id)),
        case.shape.owner == Owner::Head,
        "{}: actual Nm belongs to the original It HEAD",
        case.id
    );
    assert_eq!(
        outer.map(|node| (node.line, node.column)),
        case.ast.outer_source,
        "{}",
        case.id
    );
}

fn roundtrip(case: &Case) -> ResolvedContent {
    let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored: ResolvedContent = restored.into();
    assert_eq!(restored, original, "{}: actual JSON string", case.id);
    assert_eq!(
        mant_ir::validate_document(restored.document.as_ref().unwrap()).len(),
        0,
        "{}",
        case.id
    );
    restored
}

fn section_blocks(content: &ResolvedContent) -> &[Block] {
    &content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SYNOPSIS")
        .unwrap()
        .blocks
}

fn assert_owner(content: &ResolvedContent, case: &Case) {
    let blocks = section_blocks(content);
    if case.shape.owner == Owner::Head {
        let Block::DefinitionList { items, .. } = &blocks[0] else {
            panic!("{}: original definition owner", case.id)
        };
        assert_eq!(items.len(), 1, "{}", case.id);
        assert_eq!(items[0].terms.len(), 1, "{}", case.id);
        let source = items[0].source.unwrap();
        assert_eq!(
            Some((source.line, source.column)),
            case.ast.outer_source,
            "{}",
            case.id
        );
        assert_eq!(
            blocks
                .iter()
                .filter(|block| matches!(block, Block::DefinitionList { .. }))
                .count(),
            1,
            "{}",
            case.id
        );
    } else if let Some(source) = blocks.iter().find_map(|block| match block {
        Block::Paragraph { source, .. } | Block::Preformatted { source, .. } => *source,
        _ => None,
    }) {
        assert_eq!(
            (source.line, source.column),
            case.ast.nm_source,
            "{}",
            case.id
        );
    }
}

fn rows(content: &ResolvedContent) -> Vec<String> {
    let text = mant_render::render_query_man(content);
    text.split_once("SYNOPSIS\n")
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0
        .trim_end_matches('\n')
        .split('\n')
        .map(str::to_owned)
        .collect()
}

fn inline_cells(nodes: &[Inline], style: u8, output: &mut Vec<(char, u8)>) {
    for node in nodes {
        match node {
            Inline::Strong { children } => inline_cells(children, style | 1, output),
            Inline::Emphasis { children } => inline_cells(children, style | 2, output),
            Inline::Link { children, .. } => inline_cells(children, style, output),
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                output.extend(
                    value
                        .chars()
                        .filter(|ch| !ch.is_whitespace())
                        .map(|ch| (ch, style)),
                );
            }
            Inline::Anchor { .. } | Inline::LineBreak {} => {}
        }
    }
}

fn block_cells(blocks: &[Block], output: &mut Vec<(char, u8)>) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                inline_cells(children, 0, output);
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    for term in &item.terms {
                        inline_cells(term, 0, output);
                    }
                    block_cells(&item.description, output);
                }
            }
            Block::VerticalSpace { .. } => {}
            _ => panic!("unexpected original block {block:?}"),
        }
    }
}

fn append_payload(output: &mut String, text: &str) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(text);
}

fn portable_payload(blocks: &[Block], output: &mut String, list_count: &mut usize) {
    for block in blocks {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                append_payload(output, &inline_plain_text(children));
            }
            Block::List {
                kind: mant_ir::ListKind::Bullet,
                items,
                ..
            } => {
                *list_count += 1;
                assert_eq!(*list_count, 1, "one real root list owner");
                assert_eq!(items.len(), 1, "one real item");
                portable_payload(&items[0].blocks, output, list_count);
            }
            _ => panic!("unexpected actual Markdown owner {block:?}"),
        }
    }
}

fn assert_readback_owner(blocks: &[Block], case: &Case) {
    if case.shape.owner != Owner::Head {
        return;
    }
    let index = blocks
        .iter()
        .position(|block| matches!(block, Block::List { .. }))
        .unwrap();
    let before = &blocks[..index];
    if case.portable_rows[0].is_empty() {
        let [Block::Paragraph { children, .. }] = before else {
            panic!("{}: original leading row owner", case.id)
        };
        assert_eq!(inline_plain_text(children), "\n", "{}", case.id);
    } else {
        assert_eq!(before, [], "{}", case.id);
    }
    let after = &blocks[index + 1..];
    if case.font_control {
        let [Block::Paragraph { children, .. } | Block::Preformatted { children, .. }] = after
        else {
            panic!("{}: original text after the definition", case.id)
        };
        assert_eq!(
            inline_plain_text(children),
            if case.shape.fill == Fill::Nofill {
                "OUTER\nPREVIOUS"
            } else {
                "OUTER PREVIOUS"
            },
            "{}",
            case.id
        );
    } else {
        assert_eq!(after, [], "{}", case.id);
    }
}

#[test]
fn name_parts_keep_all_owned_shapes_exact_rows_seams_and_fonts() {
    // Forty complete sources ran all 200 pristine profiles before assertions.
    // mdoc_term.c::print_mdoc_node visits all direct siblings before/after
    // HEAD/BODY; termp_nm_pre/post (966-1045) distinguishes empty HEAD from
    // nonempty BODY and measures the latter from its first string or meta.name.
    // term_flushln consumes each real post once; source offsets (including ti)
    // affect the HEAD rmargin before its half-EN tail comparison (term.c:250).
    let fixture: Fixture =
        serde_json::from_str(include_str!("name_phase_matrix/cases.json")).unwrap();
    assert_eq!(
        fixture.reference_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.cases.len(), 40);
    let shapes: std::collections::BTreeSet<_> =
        fixture.cases[..32].iter().map(|case| case.shape).collect();
    assert_eq!(shapes.len(), 32, "complete independent five-axis matrix");
    let mut differences = Vec::new();
    for case in fixture.cases {
        assert_ast(&case);
        let content = roundtrip(&case);
        assert_owner(&content, &case);
        // Exact native physical rows, with only the reference's common five
        // column base removed. No word-space run or hard row is folded.
        let actual = rows(&content);
        if actual != case.rows {
            differences.push(format!("{}: {actual:?} != {:?}", case.id, case.rows));
        }
        let mut cells = Vec::new();
        block_cells(section_blocks(&content), &mut cells);
        assert_eq!(
            cells, case.font_cells,
            "{}: native glyph/font stream",
            case.id
        );
    }
    assert_eq!(
        differences,
        Vec::<String>::new(),
        "all native row differences"
    );
}

#[test]
fn name_part_matrix_survives_real_markdown_owner_readback() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("name_phase_matrix/cases.json")).unwrap();
    let mut differences = Vec::new();
    for case in fixture.cases {
        let original = roundtrip(&case);
        let markdown = mant_codec::encode::render_markdown(&original);
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        assert_readback_owner(section_blocks(&read), &case);
        let mut payload = String::new();
        let mut lists = 0;
        // Read the actual list item. Its marker and native preferred BODY
        // alignment are semantic owner presentation, not payload characters.
        // Joining actual block contents excludes inter-block presentation
        // gaps while preserving every authored hard/empty row in those roots.
        portable_payload(section_blocks(&read), &mut payload, &mut lists);
        assert_eq!(
            lists,
            usize::from(case.shape.owner == Owner::Head),
            "{}",
            case.id
        );
        // The native row extraction also excludes the final section gap.
        // Leading and interior hard/empty rows remain exact.
        let actual: Vec<_> = payload
            .trim_end_matches('\n')
            .split('\n')
            .map(str::to_owned)
            .collect();
        if actual != case.portable_rows {
            differences.push(format!(
                "{}: {actual:?} != {:?}\n{markdown}",
                case.id, case.portable_rows
            ));
        }
    }
    assert_eq!(
        differences,
        Vec::<String>::new(),
        "all actual readback differences"
    );
}
