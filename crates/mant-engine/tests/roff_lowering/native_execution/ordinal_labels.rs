//! The complete source label must survive whenever a generated marker loses information.

use std::fmt::Write as _;

use mant_ir::{
    Block, DefinitionItem, EquationAtomRole, EquationKind, Inline, LinkTarget, ListKind,
};
use mant_protocol::ContentSelector;

mod quoted_labels;

const MDOC: &str = concat!(
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n",
    ".Nm probe\n.Nd exercise descriptors\n.Sh DESCRIPTION\n",
);
const MAN: &str = ".TH PROBE 1 \"November 18, 2026\"\n.SH DESCRIPTION\n";

#[test]
fn committed_head_rows_and_fixed_spaces_prevent_ordinal_replacement() {
    // Fixed CVS termp_it_post() prints one distinct row for an explicit
    // .br/.sp 0 or a tbl data span. term_word() keeps an authored \~ cell.
    let header = MDOC.replace("exercise descriptors", "probe page");
    for family in ["br", "sp0", "nbsp", "table"] {
        for mask in 0_u8..8 {
            let mut source = format!("{header}.Bl -tag -width Ds\n");
            for number in 1..=3 {
                let active = mask & (1 << (number - 1)) != 0;
                let head = match (family, active) {
                    ("table", true) => format!("Xo\n.TS\nl.\n({number})\n.TE\n.Xc"),
                    ("table", false) => format!("Xo\n({number})\n.Xc"),
                    ("br", true) => format!("Xo\n.No {number} .\n.br\n.Xc"),
                    ("sp0", true) => format!("Xo\n.No {number} .\n.sp 0\n.Xc"),
                    ("nbsp", true) => format!(r"\~{number}."),
                    _ => format!("Xo\n.No {number} .\n.Xc"),
                };
                writeln!(source, ".It {head}\nBODY{number}").unwrap();
            }
            source.push_str(".El\n");
            let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let content = crate::json_roundtrip::json_string_roundtrip(&loaded);
            let [block] = content.document.as_ref().unwrap().sections[1]
                .blocks
                .as_slice()
            else {
                panic!("{source}");
            };
            if mask == 0 {
                assert!(
                    matches!(block, Block::List { kind: ListKind::Ordered { start: Some(1) }, items, .. } if items.len() == 3),
                    "{source}"
                );
                continue;
            }
            let Block::DefinitionList { items, .. } = block else {
                panic!("{source}\n{block:?}");
            };
            assert_eq!(items.len(), 3, "{source}");
            let rendered = mant_render::render_query_man_at_width(&content, 78);
            let rows: Vec<_> = rendered.lines().map(str::trim_ascii).collect();
            for (number, item) in (1..=3).zip(items) {
                let active = mask & (1 << (number - 1)) != 0;
                let label = match (family, active) {
                    ("table", _) => format!("({number})"),
                    ("nbsp", true) => format!("\u{a0}{number}."),
                    _ => format!("{number}."),
                };
                assert_eq!(
                    mant_ir::inline_plain_text(&item.terms[0]),
                    label,
                    "{source}"
                );
                let body = format!("BODY{number}");
                assert_eq!(
                    mant_ir::inline_plain_text(paragraph(&item.description)),
                    body
                );
                let body_row = rows.iter().position(|row| row.ends_with(&body)).unwrap();
                if active && family != "nbsp" {
                    assert_eq!(
                        &rows[body_row - 1..=body_row],
                        &[label.as_str(), body.as_str()],
                        "{source}\n{rendered}"
                    );
                } else {
                    assert!(rows[body_row].starts_with(&label), "{source}\n{rendered}");
                }
            }
        }
    }
}

#[test]
fn man_ordinal_heads_keep_fixed_spaces_in_both_tag_macros() {
    for (before, label) in [(".IP \"", r#"\~{number}.""#), (".TP\n.B ", r"\~{number}.")] {
        let mut source = MAN.to_owned();
        for number in 1..=3 {
            let label = label.replace("{number}", &number.to_string());
            writeln!(source, "{before}{label}\nBODY{number}").unwrap();
        }
        let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let [Block::DefinitionList { items, .. }] = content.document.as_ref().unwrap().sections[0]
            .blocks
            .as_slice()
        else {
            panic!("{source}");
        };
        assert_eq!(items.len(), 3);
        for (number, item) in (1..=3).zip(items) {
            assert_eq!(
                mant_ir::inline_plain_text(&item.terms[0]),
                format!("\u{a0}{number}.")
            );
            assert_eq!(
                mant_ir::inline_plain_text(paragraph(&item.description)),
                format!("BODY{number}")
            );
        }
    }
}

#[derive(Clone, Copy)]
enum Payload {
    Equation,
    SectionLink,
}

impl Payload {
    fn label(self, number: u8, active: bool) -> String {
        match (self, active) {
            (Self::Equation, true) => format!("Xo\n.EQ\n({number})\n.EN\n.Xc"),
            (Self::Equation, false) => format!("({number})"),
            (Self::SectionLink, true) => format!("Sx {number} ."),
            (Self::SectionLink, false) => format!("{number}."),
        }
    }
}

#[test]
fn any_semantic_ordinal_payload_keeps_the_entire_definition_group() {
    // Fixed CVS mdoc_it_pre()/print_mdoc_node() prints one distinct dt/dd
    // owner per item. HTML retains the math or Sx object at each bit of the
    // mask. Printable numbers in the other items are independent controls.
    for payload in [Payload::Equation, Payload::SectionLink] {
        for mask in 0_u8..8 {
            let source = mdoc_list(
                "tag",
                (1..=3).map(|number| {
                    let active = mask & (1 << (number - 1)) != 0;
                    payload.label(number, active)
                }),
            );
            let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let query = crate::json_roundtrip::json_string_roundtrip(&loaded);
            let doc = query.document.as_ref().unwrap();
            let [block] = doc.sections[1].blocks.as_slice() else {
                panic!("{source}");
            };
            if mask == 0 {
                assert!(
                    matches!(block, Block::List { kind: ListKind::Ordered { start: Some(1) }, items, .. } if items.len() == 3),
                    "{source}"
                );
                continue;
            }
            let Block::DefinitionList { items, .. } = block else {
                panic!("{source}");
            };
            assert_eq!(items.len(), 3, "{source}");
            for (index, item) in (1_u8..=3).zip(items) {
                let active = mask & (1 << (index - 1)) != 0;
                assert_item(&query, item, index, payload, active, &source);
            }
            assert_markdown_owners(&query, payload, mask, &source);
        }
    }
}

fn assert_markdown_owners(
    query: &mant_ir::ResolvedContent,
    payload: Payload,
    mask: u8,
    source: &str,
) {
    use mant_codec::encode::MarkdownOptions;
    let options = match payload {
        // Portable Markdown uses inline code for readable mathematics.
        Payload::Equation => MarkdownOptions::READABLE,
        Payload::SectionLink => MarkdownOptions::READABLE_ADDRESSABLE,
    };
    let markdown = mant_codec::encode::render_markdown_with_options(query, options);
    let reread = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let doc = reread.document.as_ref().unwrap();
    let Block::List { items, .. } = &doc.sections[1].blocks[0] else {
        panic!("{source}: {markdown}");
    };
    assert_eq!(items.len(), 3, "{source}");
    for (number, item) in (1..=3).zip(items) {
        let active = mask & (1 << (number - 1)) != 0;
        let body = paragraph(&item.blocks);
        let text = mant_ir::inline_plain_text(body);
        for owner in 1..=3 {
            assert_eq!(
                text.contains(&format!("BODY-{owner}")),
                owner == number,
                "{source}: {markdown}"
            );
        }
        let label = body.iter().any(|node| match (&payload, node) {
            (Payload::Equation, Inline::Code { value }) => value == &format!("( {number} )"),
            (
                Payload::SectionLink,
                Inline::Link {
                    target: LinkTarget::Section { id },
                    children,
                    ..
                },
            ) => {
                id.as_str() == format!("{number}-section")
                    && mant_ir::inline_plain_text(children) == number.to_string()
            }
            _ => false,
        });
        assert_eq!(label, active, "{source}: {markdown}");
    }
}

#[test]
fn ordinal_anchors_move_only_when_the_complete_label_is_replaceable() {
    for payload in [None, Some(Payload::Equation), Some(Payload::SectionLink)] {
        let source = mdoc_list(
            "tag",
            (1..=3).map(|number| match payload {
                None => format!("Xo\n.Tg Target-{number}\n.No {number}.\n.Xc"),
                Some(Payload::Equation) => {
                    format!("Xo\n.Tg Target-{number}\n.EQ\n({number})\n.EN\n.Xc")
                }
                Some(Payload::SectionLink) => {
                    format!("Xo\n.Tg Target-{number}\n.Sx {number} .\n.Xc")
                }
            }),
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let doc = query.document.as_ref().unwrap();
        assert_eq!(
            matches!(doc.sections[1].blocks[0], Block::List { .. }),
            payload.is_none()
        );
        let owners: Vec<_> = match &doc.sections[1].blocks[0] {
            Block::List { items, .. } => items
                .iter()
                .map(|item| {
                    let body = paragraph(&item.blocks);
                    (body, body, item.source)
                })
                .collect(),
            Block::DefinitionList { items, .. } => items
                .iter()
                .map(|item| {
                    (
                        item.terms[0].as_ref(),
                        paragraph(&item.description),
                        item.source,
                    )
                })
                .collect(),
            _ => panic!("{source}"),
        };
        let index = mant_ir::DocumentIndex::build(doc);
        for (number, (carrier, body, owner_source)) in (1..=3).zip(owners) {
            let exact = format!("Target-{number}");
            let id = index.fragment_target(&exact).unwrap();
            let anchors: Vec<_> = carrier
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Anchor {
                        id: actual,
                        owner_source,
                        ..
                    } if actual == id => Some(owner_source),
                    _ => None,
                })
                .collect();
            assert_eq!(anchors, [&owner_source], "{source}");
            assert_eq!(
                mant_ir::inline_plain_text(body),
                format!("BODY-{number}"),
                "{source}"
            );
        }
    }
}

fn paragraph(blocks: &[Block]) -> &[Inline] {
    let [Block::Paragraph { children, .. }] = blocks else {
        panic!("{blocks:?}");
    };
    children
}

fn assert_item(
    query: &mant_ir::ResolvedContent,
    item: &DefinitionItem,
    number: u8,
    payload: Payload,
    active: bool,
    source: &str,
) {
    let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
        panic!("{source}: {item:?}");
    };
    assert_eq!(
        mant_ir::inline_plain_text(children),
        format!("BODY-{number}"),
        "{source}"
    );
    let [term] = item.terms.as_slice() else {
        panic!("{source}: {item:?}");
    };
    let witnesses = term
        .iter()
        .filter(|inline| matches!(inline, Inline::Equation { .. } | Inline::Link { .. }))
        .collect::<Vec<_>>();
    if !active {
        assert!(witnesses.is_empty(), "{source}");
        assert_eq!(
            mant_ir::inline_plain_text(term),
            payload.label(number, false),
            "{source}"
        );
        return;
    }
    match (payload, witnesses.as_slice()) {
        (Payload::Equation, [Inline::Equation { value, expression }]) => {
            assert_eq!(value, &format!("( {number} )"), "{source}");
            assert_eq!(expression.kind, EquationKind::Sequence);
            let [left, atom, right] = expression.children.as_slice() else {
                panic!("{source}");
            };
            assert_eq!(
                (left.text.as_deref(), right.text.as_deref()),
                (Some("("), Some(")"))
            );
            assert_eq!(atom.text.as_deref(), Some(number.to_string().as_str()));
            assert_eq!(atom.atom_role, EquationAtomRole::Number);
        }
        (
            Payload::SectionLink,
            [
                Inline::Link {
                    target: LinkTarget::Section { id },
                    children,
                    ..
                },
            ],
        ) => {
            assert_eq!(mant_ir::inline_plain_text(children), number.to_string());
            let selected =
                mant_query::select_excerpt(query, &[ContentSelector::id(id.as_str())]).unwrap();
            assert!(
                mant_render::render_excerpt_text(&selected).contains(&format!("DEST-{number}")),
                "{source}"
            );
        }
        _ => panic!("{source}: {witnesses:?}"),
    }
}

fn mdoc_list(style: &str, labels: impl IntoIterator<Item = String>) -> String {
    let width = if matches!(style, "tag" | "hang") {
        " -width Ds"
    } else {
        ""
    };
    let mut source = format!("{MDOC}.Bl -{style}{width}\n");
    for (offset, label) in labels.into_iter().enumerate() {
        writeln!(source, ".It {label}\nBODY-{}", offset + 1).unwrap();
    }
    source.push_str(".El\n.Sh 1\nDEST-1\n.Sh 2\nDEST-2\n.Sh 3\nDEST-3\n");
    source
}

#[test]
fn mdoc_ordinal_marker_still_accepts_only_the_approved_source_forms() {
    for (first, second, third, mark, recovered) in [
        ("1.", "2.", "3.", "1.", true),
        ("1)", "2)", "3)", "1)", true),
        ("(1)", "(2)", "(3)", "(1)", true),
        (r"1\&.", r"2\&.", r"3\&.", "1.", true),
        (r"1\fR)", r"2\fR)", r"3\fR)", "1)", true),
        ("Sy 1 .", "Sy 2 .", "Sy 3 .", "1.", true),
        ("Pq 1", "Pq 2", "Pq 3", "(1)", true),
        ("[1]", "[2]", "[3]", "[1]", false),
        ("001.", "002.", "003.", "001.", false),
        ("+1.", "+2.", "+3.", "+1.", false),
        ("Sy 1 Li 1.", "Sy 2 Li 2.", "Sy 3 Li 3.", "1 1.", false),
    ] {
        let source = mdoc_list("tag", [first, second, third].map(str::to_owned));
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let [block] = query.document.as_ref().unwrap().sections[1]
            .blocks
            .as_slice()
        else {
            panic!("{source}");
        };
        match block {
            Block::List {
                kind: ListKind::Ordered { start: Some(1) },
                items,
                ..
            } if recovered => assert_eq!(items.len(), 3),
            Block::DefinitionList { items, .. } if !recovered => {
                assert_eq!(items.len(), 3);
                assert_eq!(mant_ir::inline_plain_text(&items[0].terms[0]), mark);
            }
            _ => panic!("{source}: {block:?}"),
        }
    }
    for style in ["tag", "hang", "inset", "ohang", "diag"] {
        let source = mdoc_list(style, ["1.", "2.", "3."].map(str::to_owned));
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let block = &query.document.as_ref().unwrap().sections[1].blocks[0];
        assert_eq!(
            matches!(block, Block::List { .. }),
            style == "tag",
            "{source}"
        );
    }
}

#[test]
fn man_ordinals_keep_tp_ip_spelling_and_register_progression() {
    // pre_TP() selects the actual next-line label. pre_IP() uses the first
    // operand and leaves the current incremented counter value in the tree.
    for (definition, retain) in [
        (".TP\n1.\nBODY-1\n.TP\n2.\nBODY-2\n", false),
        (".TP\n\\fB1\\fP.\nBODY-1\n.TP\n\\fB2\\fP.\nBODY-2\n", false),
        (".IP 1.\nBODY-1\n.IP 2.\nBODY-2\n", false),
        (".IP 1\\&.\nBODY-1\n.IP 2\\&.\nBODY-2\n", false),
        (
            ".nr cnt 0 1\n.IP \\n+[cnt]\nBODY-1\n.IP \\n+[cnt]\nBODY-2\n",
            false,
        ),
        (".IP [1]\nBODY-1\n.IP [2]\nBODY-2\n", true),
        (".IP +1.\nBODY-1\n.IP +2.\nBODY-2\n", true),
        (".IP 001.\nBODY-1\n.IP 002.\nBODY-2\n", true),
    ] {
        let source = format!("{MAN}{definition}");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let blocks = &query.document.as_ref().unwrap().sections[0].blocks;
        if retain {
            assert!(
                matches!(blocks.as_slice(), [Block::DefinitionList { items, .. }] if items.len() == 2),
                "{source}: {blocks:?}"
            );
        } else {
            assert!(
                matches!(blocks.as_slice(), [Block::List { kind: ListKind::Ordered { start: Some(1) }, items, .. }] if items.len() == 2),
                "{source}: {blocks:?}"
            );
        }
        let visible = mant_render::render_query_text(&query);
        for body in ["BODY-1", "BODY-2"] {
            assert_eq!(visible.matches(body).count(), 1, "{source}");
        }
    }
}
