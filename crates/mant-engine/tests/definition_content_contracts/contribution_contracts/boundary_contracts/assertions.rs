use super::*;

#[derive(Default)]
struct Types {
    literals: Vec<(String, Option<String>)>,
    body: Vec<String>,
    lists: usize,
    rules: usize,
}

impl<'ir> visit::Visit<'ir> for Types {
    fn visit_block(&mut self, block: &'ir Block) {
        match block {
            Block::Preformatted {
                children, language, ..
            } => self
                .literals
                .push((inline_plain_text(children), language.clone())),
            Block::Paragraph { children, .. } => {
                let value = inline_plain_text(children);
                if value.contains(BODY) {
                    self.body.push(value);
                }
            }
            Block::List { .. } => self.lists += 1,
            Block::ThematicBreak { .. } => self.rules += 1,
            _ => {}
        }
        visit::walk_block(self, block);
    }
}

fn types(value: &ResolvedContent) -> Types {
    let mut output = Types::default();
    visit::Visit::visit_document(&mut output, value.document.as_ref().unwrap());
    output
}

pub(super) fn assert_receiver(value: &ResolvedContent, physical: Physical) {
    let found = types(value);
    let literals = if matches!(physical, Physical::Fence) {
        vec![("Contribution中\nLiteralEnd".into(), Some("contract".into()))]
    } else {
        vec![]
    };
    assert_eq!(
        found.literals, literals,
        "real fence payloads survive intact"
    );
    assert_eq!(found.body.len(), usize::from(physical != Physical::Fence));
    if let Some(body) = found.body.first() {
        assert!(
            body.ends_with(BODY) || body.ends_with(&format!("{BODY}\n\n")),
            "exact receiver payload and an optional completed EOF gap: {body:?}"
        );
    }
    assert_eq!(found.rules, usize::from(matches!(physical, Physical::Rule)));
    if matches!(physical, Physical::List) {
        assert!(found.lists >= 1, "the receiver remains a real list");
    }
    assert_eq!(
        mant_render::render_query_man(value).matches(BODY).count(),
        1
    );
}

fn content_blocks(value: &ResolvedContent, context: Context) -> &[Block] {
    let document = value.document.as_ref().unwrap();
    if matches!(context, Context::Root) {
        &document.blocks
    } else {
        &document.sections[0].blocks
    }
}

enum Row {
    Gap,
    Text(String),
    Rule,
}

fn rows(blocks: &[Block], output: &mut Vec<Row>) {
    for block in blocks {
        if geometry::block_gap(block) > 0 {
            output.push(Row::Gap);
        }
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                let text = inline_plain_text(children);
                if !text.is_empty() {
                    output.push(Row::Text(text));
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    if item.layout.spacing_before_lines.is_some_and(|gap| gap > 0) {
                        output.push(Row::Gap);
                    }
                    rows(&item.blocks, output);
                }
            }
            Block::ThematicBreak { .. } => output.push(Row::Rule),
            Block::VerticalSpace { .. } => {}
            _ => panic!("unexpected imported block: {block:#?}"),
        }
    }
}

fn assert_leading_gap(value: &ResolvedContent, context: Context, physical: Physical) {
    let mut ledger = vec![];
    rows(content_blocks(value, context), &mut ledger);
    let first = ledger
        .iter()
        .position(|row| match row {
            Row::Text(value) => value.contains(BODY),
            Row::Rule => matches!(physical, Physical::Rule),
            Row::Gap => false,
        })
        .unwrap();
    assert!(
        ledger[..=first].iter().any(|row| {
            matches!(row, Row::Gap) || matches!(row, Row::Text(value) if value.starts_with('\n'))
        }),
        "leading spacing owns a physical empty row before {physical:?}"
    );
}

pub(super) fn assert_gap(
    value: &ResolvedContent,
    context: Context,
    position: Position,
    physical: Physical,
) {
    let plain = mant_render::render_query_man(value);
    let receiver = plain
        .find(if matches!(physical, Physical::Rule) {
            "---"
        } else {
            BODY
        })
        .unwrap();
    match position {
        Position::Prefix if matches!(context, Context::Root | Context::Section) => {
            assert_leading_gap(value, context, physical);
        }
        Position::Prefix | Position::Middle => {
            let word = if matches!(position, Position::Prefix) {
                NAME
            } else {
                BEFORE
            };
            let before = plain.find(word).unwrap() + word.len();
            assert!(
                plain[before..receiver].matches('\n').count() >= 2,
                "{plain}"
            );
        }
        Position::Tail => {
            let word = if matches!(physical, Physical::Fence) {
                "LiteralEnd"
            } else {
                BODY
            };
            let end = plain.find(word).unwrap() + word.len();
            assert!(
                plain[end..].matches('\n').count() >= 2,
                "completed EOF gap: {plain}"
            );
        }
        Position::TailFollow => unreachable!(),
    }
    assert_eq!(
        plain.matches(BEFORE).count(),
        usize::from(!matches!(position, Position::Prefix))
    );
    assert_eq!(
        plain.matches(AFTER).count(),
        usize::from(!matches!(position, Position::Tail))
    );
}

pub(super) fn assert_only_spacing(value: &ResolvedContent, context: Context) {
    let found = types(value);
    assert!(
        found.literals.is_empty(),
        "spacing has no authored code payload"
    );
    assert_eq!(
        found.lists, 0,
        "no body contribution means no fabricated list marker"
    );
    assert_eq!(found.rules, 0);
    assert_eq!(found.body, [] as [String; 0]);
    let mut ledger = vec![];
    rows(content_blocks(value, context), &mut ledger);
    assert!(
        ledger
            .iter()
            .any(|row| matches!(row, Row::Gap) || matches!(row, Row::Text(value) if value == "\n")),
        "even without a receiver, positive spacing retains an empty physical row"
    );
    for row in ledger {
        if let Row::Text(value) = row {
            assert_eq!(value, "\n", "spacing does not invent body scalars");
        }
    }
}

pub(super) fn assert_root_gap_once(value: &ResolvedContent, prefix: bool) {
    let plain = mant_render::render_query_man(value);
    let expected = if prefix {
        format!("Probe\n\n\n{BODY}")
    } else {
        format!("Probe\n\n{BODY}\n\n")
    };
    assert_eq!(plain, expected, "one accepted gap row is consumed once");
}

pub(super) fn assert_empty_table(value: &ResolvedContent, shape: EmptyTable) {
    let found = types(value);
    let expected = if matches!(shape, EmptyTable::NoRows) {
        vec![]
    } else {
        vec![(String::new(), None)]
    };
    assert_eq!(
        found.literals, expected,
        "one empty Data row owns one empty fence"
    );
    let [body] = found.body.as_slice() else {
        panic!("one independent prose receiver")
    };
    // A zero-row table may be omitted next to HEAD. A real empty Data row is
    // a structural fence receiver and cannot consume the following prose.
    if matches!(shape, EmptyTable::NoRows) {
        assert!(body.ends_with(BODY));
    } else {
        assert_eq!(body, BODY);
    }
    assert_eq!(
        mant_render::render_query_man(value).matches(BODY).count(),
        1
    );
}
