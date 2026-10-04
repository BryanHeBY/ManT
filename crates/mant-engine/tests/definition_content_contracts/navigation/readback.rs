use super::*;
use fixtures::{Body, Case, Head};

pub(super) fn shape(blocks: &[Block]) -> Vec<String> {
    let mut output = vec![];
    let mut pending = 0_u16;
    for block in blocks {
        // Markdown may attach the same resolved gap to a transparent
        // paragraph or to its next physical block. Transfer, never discard,
        // that gap; the exact rendered row ledger is checked independently.
        let gap = geometry::block_gap(block).saturating_add(pending);
        if matches!(block, Block::VerticalSpace { .. })
            || matches!(block, Block::Paragraph { children, .. } if inline_plain_text(children).is_empty())
        {
            pending = gap;
        } else {
            append_shape(block, &mut output, gap);
            pending = 0;
        }
    }
    if pending > 0 {
        output.push(format!("gap/{pending}"));
    }
    output
}

fn append_shape(block: &Block, output: &mut Vec<String>, gap: u16) {
    match block {
        Block::Paragraph { children, .. } => {
            let text = inline_plain_text(children);
            if !text.is_empty() || gap > 0 {
                output.push(format!("prose/{gap}:{text:?}"));
            }
        }
        Block::Preformatted {
            children, language, ..
        } => {
            output.push(format!(
                "literal/{gap}/{language:?}:{:?}",
                inline_plain_text(children)
            ));
        }
        Block::List {
            items,
            kind,
            compact,
            ..
        } => {
            output.push(format!("list/{gap}/{kind:?}/{compact}"));
            for item in items {
                output.push(format!("item/{:?}", item.layout.spacing_before_lines));
                output.extend(shape(&item.blocks));
                output.push("/item".into());
            }
            output.push("/list".into());
        }
        Block::VerticalSpace { lines, .. } => output.push(format!("gap/{lines}")),
        Block::ThematicBreak { .. } => output.push(format!("rule/{gap}")),
        Block::Unsupported { name, text, .. } => {
            output.push(format!("unsupported/{gap}/{name:?}:{text:?}"));
        }
        // The source BODY uses these types, but their documented Markdown
        // projection is a fence. A changed readback type cannot hide here.
        _ => output.push(format!("unexpected:{block:?}")),
    }
}

fn reader(markdown: &str) -> ResolvedContent {
    mant_loader::load_markdown_text(markdown, None).unwrap()
}

fn definition_blocks(value: &ResolvedContent) -> &[Block] {
    let sections = &value.document.as_ref().unwrap().sections;
    let [Block::List { items, .. }, Block::Paragraph { children, .. }] =
        sections[0].blocks.as_slice()
    else {
        panic!(
            "one definition list and an independent Tail: {:#?}",
            sections[0].blocks
        )
    };
    assert_eq!(inline_plain_text(children), TAIL);
    assert_eq!(items.len(), 1, "navigation cannot create a second item");
    &items[0].blocks
}

fn assert_body_type(blocks: &[Block], body: Body) {
    match body {
        Body::Paragraph => assert_eq!(blocks.iter().filter(|block| matches!(block, Block::Paragraph { children, .. } if inline_plain_text(children).contains(BODY))).count(), 1),
        Body::List | Body::Ordered => {
            let lists = blocks.iter().filter_map(|block| {
                if let Block::List { items, kind, .. } = block { Some((items, kind)) } else { None }
            }).collect::<Vec<_>>();
            assert_eq!(lists.len(), 1);
            let kind = if body == Body::Ordered { ListKind::Ordered { start: Some(2) } } else { ListKind::Bullet };
            assert_eq!(*lists[0].1, kind);
            assert_eq!(lists[0].0.len(), 1);
            assert!(matches!(
                lists[0].0[0].blocks.as_slice(),
                [Block::Paragraph { children, layout, .. }]
                    if layout.spacing_before_lines == 0
                        && inline_plain_text(children).ends_with(BODY)
                        && inline_plain_text(children).matches(BODY).count() == 1
            ));
            // Preserved raw <a> tags follow the reader's literal HTML policy.
            // Exact prefix/body text is checked against the same-anchor
            // baseline below; they cannot change this paragraph's block type.
        }
        Body::Literal | Body::Table | Body::Equation => {
            let payloads = blocks.iter().filter_map(|block| {
                if let Block::Preformatted { children, language, .. } = block {
                    Some((inline_plain_text(children), language.as_deref()))
                } else { None }
            }).collect::<Vec<_>>();
            let expected = match body {
                Body::Literal => ("Body中\nLiteralTail", Some("txt")),
                Body::Table => ("Body中 | CellEnd", None),
                Body::Equation => (BODY, Some("math")),
                _ => unreachable!(),
            };
            assert_eq!(payloads, [(expected.0.into(), expected.1)]);
        }
        Body::Thematic => {
            assert_eq!(blocks.iter().filter(|block| matches!(block, Block::ThematicBreak { .. })).count(), 1);
            assert_eq!(blocks.iter().filter(|block| matches!(block, Block::Paragraph { children, .. } if inline_plain_text(children) == BODY)).count(), 1);
        }
    }
}

fn assert_explicit_rows(value: &ResolvedContent, case: Case, markdown: &str) {
    let plain = mant_render::render_query_man(value);
    assert_eq!(
        plain.matches(BODY).count(),
        1,
        "{case:?}: {markdown}\n{plain}"
    );
    assert_eq!(plain.matches(TAIL).count(), 1);
    if !case.head.has_word() && case.spacing > 0 {
        let blocks = definition_blocks(value);
        assert!(
            matches!(
                blocks.first(), Some(Block::Paragraph { children, .. })
                    if inline_plain_text(children) == "\n"
            ),
            "the label-free marker frames an executed gap: {case:?}: {markdown}"
        );
        // The next completed boundary is retained even when an empty link
        // transports it to another root. Do not derive this from baseline.
        let first = blocks[1..]
            .iter()
            .find(|block| geometry::block_gap(block) > 0);
        assert_eq!(
            first.map(geometry::block_gap),
            Some(1),
            "{case:?}: {markdown}"
        );
        assert!(
            plain[plain.find('•').unwrap()..plain.find(BODY).unwrap()]
                .matches('\n')
                .count()
                >= 2,
            "positive BODY spacing cannot disappear: {case:?}: {markdown}"
        );
    }
    let tail = plain.find(TAIL).unwrap();
    let end = plain.find(case.body.last_word()).unwrap() + case.body.last_word().len();
    assert_eq!(
        plain[end..tail].matches('\n').count(),
        2,
        "Tail boundary: {case:?}: {markdown}\n{plain}"
    );
    if case.body == Body::Paragraph && case.head.has_word() {
        let head = plain.find(NAME).unwrap() + NAME.len();
        let body = plain.find(BODY).unwrap();
        // Positive source spacing is one completed blank row in prose
        // readback (markdown/layout.rs::normalize_blocks). Closing its HEAD
        // paragraph retires a provisional empty tail; it is not another blank.
        let breaks = if case.spacing > 0 {
            2
        } else if case.relation == HeadBodyRelation::Separate {
            1 + usize::from(case.head == Head::Hard)
        } else {
            usize::from(case.head == Head::Hard)
        };
        assert_eq!(
            plain[head..body].matches('\n').count(),
            breaks,
            "HEAD/BODY boundary: {case:?}: {markdown}\n{plain}"
        );
    }
}

pub(super) fn links(value: &ResolvedContent) -> Vec<(LinkTarget, String)> {
    let mut links = vec![];
    let report = scan_references(
        value.document.as_ref().unwrap(),
        ReferenceScanLimits::default(),
        |link| {
            links.push((link.target.clone(), inline_plain_text(link.label)));
            std::ops::ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    links
}

pub(super) fn assert_reader(actual: &str, baseline: &str, case: Case, options: MarkdownOptions) {
    let imported = reader(actual);
    let reference = reader(baseline);
    let actual_blocks = &imported.document.as_ref().unwrap().sections[0].blocks;
    let expected_blocks = &reference.document.as_ref().unwrap().sections[0].blocks;
    // Navigation has zero body scalars. Ignore only such prose with zero
    // gap; every hard row, author space and positive gap stays in this tree.
    assert_eq!(
        shape(actual_blocks),
        shape(expected_blocks),
        "{case:?}/{options:?}\nactual:\n{actual}\nbaseline:\n{baseline}"
    );
    assert_eq!(
        mant_render::render_query_man(&imported),
        mant_render::render_query_man(&reference),
        "{case:?}/{options:?}\n{actual}\n{baseline}"
    );
    assert_body_type(definition_blocks(&imported), case.body);
    assert_explicit_rows(&imported, case, actual);
    let expected = case
        .navigation
        .targets()
        .into_iter()
        .filter(|target| matches!(target, LinkTarget::External { .. }))
        .map(|target| (target, String::new()))
        .collect::<Vec<_>>();
    assert_eq!(
        links(&imported),
        expected,
        "Manual omission is format policy; emitted External labels stay empty: {actual}"
    );
    for target in case.navigation.targets() {
        if let LinkTarget::External { uri } = target {
            assert_eq!(actual.matches(&uri).count(), 1, "{actual}");
        }
    }
    for anchor in ["nav-before", "Nav.Before", "nav-after"] {
        let count = usize::from(options.preserve_anchors && case.navigation.has_anchors());
        assert_eq!(
            actual.matches(&format!("id=\"{anchor}\"")).count(),
            count,
            "{actual}"
        );
    }
    let head_anchor = usize::from(options.preserve_anchors && case.head == Head::Anchor);
    assert_eq!(actual.matches("id=\"head-only\"").count(), head_anchor);
}
