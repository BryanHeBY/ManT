//! Original HEAD ownership is independent of Markdown's bullet spelling.
use super::*;

fn item(term: Vec<Inline>, description: Vec<Block>, relation: HeadBodyRelation) -> Block {
    Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![term.into()],
            description,
            head_body_relation: relation,
            layout: DefinitionLayout {
                body_indent_columns: 0,
                ..Default::default()
            },
            source: Some(source(10)),
            entry: Some(facts(EntryInlineRoot::Term { index: 0 })),
        }],
        declaration_groups: vec![],
        compact: true,
        layout: LayoutHint::default(),
        source: Some(source(10)),
    }
}

pub(super) fn content(
    context: Context,
    leaf_kind: Leaf,
    hard_rows: usize,
    positive_gap: bool,
    linked: bool,
) -> ResolvedContent {
    let mut term = vec![Inline::Strong {
        children: vec![text(NAME)],
    }];
    term.extend(std::iter::repeat_with(Inline::line_break).take(hard_rows));
    let mut description = vec![];
    if linked {
        description.push(navigation());
    }
    if positive_gap {
        description.push(gap());
    }
    description.push(leaf(leaf_kind));
    let block = item(term, description, HeadBodyRelation::Separate);
    let blocks = if matches!(context, Context::Nested) {
        vec![list(ListKind::Bullet, vec![block], false)]
    } else {
        vec![block]
    };
    install(Context::Root, blocks)
}

pub(super) fn expected(
    value: &ResolvedContent,
    context: Context,
    leaf: Leaf,
    hard_rows: usize,
    positive_gap: bool,
) -> String {
    let mut expected = reference_reading(value);
    if matches!(leaf, Leaf::Plain | Leaf::Fence | Leaf::Rule) {
        let payload = if leaf == Leaf::Rule { "---" } else { BODY };
        let (prefix, indent) = if matches!(context, Context::Nested) {
            ("Probe\n\n•\n  • ", "    ")
        } else {
            ("Probe\n\n• ", "  ")
        };
        // A separate HEAD owns all of its rows, including its final empty
        // logical row. BODY needs its own close. A requested gap adds one
        // completed row; an ordinary structural boundary adds none.
        let literal = format!(
            "{prefix}{NAME}{}{indent}{payload}",
            "\n".repeat(hard_rows + 1 + usize::from(positive_gap))
        );
        assert_eq!(expected, literal, "independent HEAD row gold");
        let (native_prefix, native_indent) = if matches!(context, Context::Nested) {
            ("Probe\n\n•\n  ", "  ")
        } else {
            ("Probe\n\n", "")
        };
        let original = format!(
            "{native_prefix}{NAME}{}{native_indent}{payload}",
            "\n".repeat(hard_rows + 1 + usize::from(positive_gap))
        );
        assert_eq!(reading(value), original, "original Definition rows");
    }
    if leaf == Leaf::Ordered {
        let (prefix, indent) = if matches!(context, Context::Nested) {
            ("Probe\n\n•\n  ", "  ")
        } else {
            ("Probe\n\n", "")
        };
        assert_eq!(
            reading(value),
            format!(
                "{prefix}{NAME}{}{indent}2. {BODY}",
                "\n".repeat(hard_rows + 1 + usize::from(positive_gap))
            ),
            "original ordered BODY rows"
        );
        if hard_rows == 0 && !positive_gap {
            // A non-1 ordered marker cannot interrupt the HEAD paragraph.
            // Only this zero-row, zero-gap seam needs its minimum grammar
            // blank. Completed HEAD rows and positive space already supply it.
            let marker = expected.find("2. ").unwrap();
            let row_start = expected[..marker].rfind('\n').unwrap() + 1;
            expected.insert(row_start, '\n');
        }
    }
    expected
}

pub(super) fn adjacent_cases() -> Vec<(String, ResolvedContent, String, Leaf)> {
    let mut cases = vec![];
    for inline_gap in [false, true] {
        let mut body = literal(vec![text(BODY)]);
        let mut blocks = vec![];
        if inline_gap {
            let Block::Preformatted { layout, .. } = &mut body else {
                unreachable!()
            };
            layout.spacing_before_lines = 1;
        } else {
            blocks.push(gap());
        }
        blocks.push(body);
        let value = install(Context::Root, vec![list(ListKind::Bullet, blocks, true)]);
        assert_eq!(reading(&value), format!("Probe\n\n•\n\n  {BODY}"));
        // A generated marker is a real row, then a completed blank precedes
        // the first literal row. The marker's close cannot consume that gap.
        cases.push((
            format!("list/leading-distance/literal-layout={inline_gap}"),
            value,
            // The imported hard-only marker paragraph owns its intrinsic
            // marker separator; source first-gap List uses a bare marker.
            // This is a generated framing cell, never author whitespace.
            format!("Probe\n\n• \n\n  {BODY}"),
            Leaf::Fence,
        ));
    }
    for leaf_kind in [Leaf::Plain, Leaf::Fence] {
        let value = install(
            Context::Root,
            vec![item(
                vec![Inline::line_break()],
                vec![paragraph(vec![Inline::line_break()]), leaf(leaf_kind)],
                HeadBodyRelation::joined(),
            )],
        );
        // HEAD and the first BODY paragraph share one inline execution root.
        // Both hard rows precede later content. Their syntax is not an
        // automatic block-distance request.
        cases.push((
            format!("definition/joined-hard-only/{leaf_kind:?}"),
            value,
            format!("Probe\n\n• \n\n  {BODY}"),
            leaf_kind,
        ));
    }
    cases
}
