//! A terminal open delimiter and an executed empty row are different facts.
use super::*;

#[derive(Clone, Copy, Debug)]
enum Suffix {
    None,
    ZeroGap,
    Gap,
    NestedGap,
}

pub(super) struct Case {
    pub(super) label: String,
    pub(super) value: ResolvedContent,
    pub(super) native: String,
    pub(super) portable: String,
    pub(super) leaf: Leaf,
}

fn head(hard_rows: usize, description: Vec<Block>, entry: bool) -> Block {
    let mut children = vec![Inline::Strong {
        children: vec![text(NAME)],
    }];
    children.extend(std::iter::repeat_with(Inline::line_break).take(hard_rows));
    Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![children.into()],
            description,
            head_body_relation: HeadBodyRelation::Separate,
            layout: DefinitionLayout {
                body_indent_columns: 0,
                ..Default::default()
            },
            source: Some(source(10)),
            entry: entry.then(|| facts(EntryInlineRoot::Term { index: 0 })),
        }],
        declaration_groups: vec![],
        compact: true,
        layout: LayoutHint::default(),
        source: Some(source(10)),
    }
}

fn body(suffix: Suffix) -> Vec<Block> {
    let space = |lines| Block::VerticalSpace {
        lines,
        source: Some(source(13)),
    };
    match suffix {
        Suffix::None => vec![],
        Suffix::ZeroGap => vec![space(0)],
        Suffix::Gap => vec![space(1)],
        Suffix::NestedGap => {
            let mut carrier = head(0, vec![space(1)], false);
            let Block::DefinitionList { items, .. } = &mut carrier else {
                unreachable!()
            };
            items[0].terms.clear();
            vec![carrier]
        }
    }
}

pub(super) fn cases() -> Vec<Case> {
    let mut cases = vec![];
    for context in [Context::Root, Context::Nested] {
        for hard_rows in 0..=2 {
            for suffix in [
                Suffix::None,
                Suffix::ZeroGap,
                Suffix::Gap,
                Suffix::NestedGap,
            ] {
                let block = head(hard_rows, body(suffix), true);
                let blocks = if matches!(context, Context::Nested) {
                    vec![list(ListKind::Bullet, vec![block], false)]
                } else {
                    vec![block]
                };
                let native_prefix = if matches!(context, Context::Nested) {
                    "Probe\n\n•\n  "
                } else {
                    "Probe\n\n"
                };
                let portable_prefix = if matches!(context, Context::Nested) {
                    "Probe\n\n•\n  • "
                } else {
                    "Probe\n\n• "
                };
                let gap = matches!(suffix, Suffix::Gap | Suffix::NestedGap);
                let native_rows = hard_rows + if gap { 2 } else { 0 };
                // One uncompleted terminal delimiter does not assert an empty
                // physical row. CommonMark may omit it. Completed EOF blank
                // rows (>=2 delimiters) remain exact through both cycles.
                let portable_rows = if native_rows == 1 { 0 } else { native_rows };
                cases.push(Case {
                    label: format!("{context:?}/head-only/{hard_rows}/{suffix:?}"),
                    value: install(Context::Root, blocks),
                    native: format!("{native_prefix}{NAME}{}", "\n".repeat(native_rows)),
                    portable: format!("{portable_prefix}{NAME}{}", "\n".repeat(portable_rows)),
                    leaf: Leaf::Rule,
                });
            }
        }
    }
    for leaf_kind in [Leaf::Plain, Leaf::Fence] {
        for hard_rows in 0..=2 {
            for positive_gap in [false, true] {
                let mut successor = vec![];
                if positive_gap {
                    successor.push(gap());
                }
                successor.push(leaf(leaf_kind));
                let mut blocks = vec![head(hard_rows, vec![], true)];
                blocks.extend(successor);
                let seam = "\n".repeat(hard_rows + 1 + usize::from(positive_gap));
                let portable_rows = hard_rows + 1 + usize::from(positive_gap);
                // Detached root prose needs the documented CommonMark scope
                // separator to remain outside the preceding bullet item. A
                // root fence interrupts that item without the extra blank.
                let portable_rows = if leaf_kind == Leaf::Plain {
                    portable_rows.max(2)
                } else {
                    portable_rows
                };
                cases.push(Case {
                    label: format!("head-only/successor/{leaf_kind:?}/{hard_rows}/{positive_gap}"),
                    value: install(Context::Root, blocks),
                    native: format!("Probe\n\n{NAME}{seam}{BODY}"),
                    portable: format!("Probe\n\n• {NAME}{}{BODY}", "\n".repeat(portable_rows)),
                    leaf: leaf_kind,
                });
            }
        }
    }
    cases
}

pub(super) fn siblings() -> Vec<Case> {
    let mut cases = vec![];
    for context in [Context::Root, Context::Nested] {
        for hard_rows in 0..=2 {
            for positive_gap in [false, true] {
                let mut block = head(hard_rows, vec![], true);
                let Block::DefinitionList { items, .. } = &mut block else {
                    unreachable!()
                };
                let Block::DefinitionList {
                    items: mut next, ..
                } = head(0, vec![], false)
                else {
                    unreachable!()
                };
                next[0].terms = vec![vec![text(AFTER)].into()];
                next[0].layout.spacing_before_lines = Some(u16::from(positive_gap));
                items.extend(next);
                let blocks = if matches!(context, Context::Nested) {
                    vec![list(ListKind::Bullet, vec![block], false)]
                } else {
                    vec![block]
                };
                let (native_prefix, native_next, portable_prefix, portable_next) =
                    if matches!(context, Context::Nested) {
                        ("Probe\n\n•\n  ", "  ", "Probe\n\n•\n  • ", "  • ")
                    } else {
                        ("Probe\n\n", "", "Probe\n\n• ", "• ")
                    };
                let seam = "\n".repeat(hard_rows + 1 + usize::from(positive_gap));
                cases.push(Case {
                    label: format!("{context:?}/sibling-heads/{hard_rows}/{positive_gap}"),
                    value: install(Context::Root, blocks),
                    native: format!("{native_prefix}{NAME}{seam}{native_next}{AFTER}"),
                    portable: format!("{portable_prefix}{NAME}{seam}{portable_next}{AFTER}"),
                    leaf: Leaf::Rule,
                });
            }
        }
    }
    cases
}
