//! A transparent RS keeps paragraph predecessor evidence across IR ownership.

use mant_ir::{Block, Inline, LayoutHint, ListKind, ResolvedContent};
use mant_loader::{load_markdown_text, load_roff_bytes};
use mant_protocol::QueryBundle;
use mant_render::render_query_text;

fn paragraph_layout<'a>(blocks: &'a [Block], text: &str) -> Option<&'a LayoutHint> {
    for block in blocks {
        match block {
            Block::Paragraph {
                children, layout, ..
            } if children
                .iter()
                .any(|inline| matches!(inline, Inline::Text { value } if value.contains(text))) =>
            {
                return Some(layout);
            }
            Block::List { items, .. } => {
                for item in items {
                    if let Some(layout) = paragraph_layout(&item.blocks, text) {
                        return Some(layout);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn check(source: &str, expected: &[(&str, u16)]) {
    let query = load_roff_bytes(source.as_bytes()).unwrap();
    let document = query.document.as_ref().unwrap();
    let text = render_query_text(&query);
    if source.contains(".IP 1.") {
        let Some(Block::List { items, .. }) = document.sections[0].blocks.first() else {
            panic!("expected an ordered owner: {document:#?}")
        };
        assert_eq!(items.len(), 1);
        for &(token, _) in expected {
            assert!(
                paragraph_layout(&items[0].blocks, token).is_some(),
                "continuation escaped its item: {document:#?}"
            );
        }
    }
    for &(token, gap) in expected {
        let layout = paragraph_layout(&document.sections[0].blocks, token)
            .unwrap_or_else(|| panic!("missing {token}: {document:#?}"));
        assert_eq!(layout.spacing_before_lines, gap, "{source}\n{text}");
        let rows: Vec<_> = text.lines().collect();
        let index = rows.iter().position(|line| line.trim() == token).unwrap();
        let blanks = rows[..index]
            .iter()
            .rev()
            .take_while(|line| line.is_empty())
            .count();
        assert_eq!(blanks, usize::from(gap), "{source}\n{text}");
    }
}

#[test]
fn ordered_item_relative_scope_applies_pd_only_for_an_explicit_paragraph() {
    // mandoc print_bvspace climbs transparent RS ancestors; it does not
    // require the predecessor to share the temporary IR output buffer.
    for pd in [0, 2] {
        for paragraph in ["", ".PP\n"] {
            let source = format!(
                ".TH PROBE 1\n.SH DESCRIPTION\n.PD {pd}\n.IP 1. 4\nFIRST\n.RS 4\n{paragraph}SECOND\n.RE\n.RS 4\n{paragraph}THIRD\n.RE\n"
            );
            let gap = if paragraph.is_empty() { 0 } else { pd };
            check(&source, &[("SECOND", gap), ("THIRD", gap)]);
        }
    }
}

#[test]
fn nested_relative_wrappers_preserve_the_ordered_item_predecessor() {
    for pd in [0, 2] {
        let source = format!(
            ".TH PROBE 1\n.SH DESCRIPTION\n.PD {pd}\n.IP 1. 4\nFIRST\n.RS 4\n.RS 2\n.PP\nSECOND\n.RE\n.RE\n"
        );
        check(&source, &[("SECOND", pd)]);
    }
}

#[test]
fn ordinary_relative_scopes_retain_the_same_paragraph_boundary_contract() {
    for pd in [0, 2] {
        for paragraph in ["", ".PP\n"] {
            let source = format!(
                ".TH PROBE 1\n.SH DESCRIPTION\n.PD {pd}\nFIRST\n.RS 4\n{paragraph}SECOND\n.RE\n.RS 4\n{paragraph}THIRD\n.RE\n"
            );
            let gap = if paragraph.is_empty() { 0 } else { pd };
            check(&source, &[("SECOND", gap), ("THIRD", gap)]);
        }
    }
}

#[test]
fn a_first_paragraph_in_a_first_relative_scope_does_not_invent_a_predecessor() {
    let source = ".TH PROBE 1\n.SH DESCRIPTION\n.PD 2\n.RS 4\n.RS 2\n.PP\nSECOND\n.RE\n.RE\n";
    let query = load_roff_bytes(source.as_bytes()).unwrap();
    let document = query.document.as_ref().unwrap();
    assert_eq!(
        paragraph_layout(&document.sections[0].blocks, "SECOND")
            .unwrap()
            .spacing_before_lines,
        0
    );
    // The heading owns its own single blank row; this first paragraph must
    // not introduce a PD=2 boundary on top of that heading presentation.
    assert!(render_query_text(&query).contains("DESCRIPTION\n      SECOND"));
}

fn transport(content: &ResolvedContent) -> ResolvedContent {
    let encoded = serde_json::to_string(&QueryBundle::from(content)).unwrap();
    let decoded: ResolvedContent = serde_json::from_str::<QueryBundle>(&encoded)
        .unwrap()
        .into();
    assert_eq!(decoded.document, content.document);
    decoded
}

fn roff_ordered_continuation(first_tag: &str, between: &str) -> String {
    format!(
        ".TH TEST 1\n.SH DESCRIPTION\n{first_tag}\nAAA\n.RS 5n\nBBB\n.RE\n{between}.RS 5n\nDDD\n.RE\n.PP\nZZZ\n"
    )
}

fn sentinel_order(text: &str) -> Vec<&str> {
    text.split(|character: char| !character.is_ascii_alphabetic())
        .filter(|word| matches!(*word, "AAA" | "BBB" | "CCC" | "DDD" | "ZZZ"))
        .collect()
}

fn sentinel_rows(text: &str) -> Vec<&str> {
    let mut lines = text.lines().skip_while(|line| !line.contains("AAA"));
    let mut rows: Vec<_> = lines
        .by_ref()
        .map(|line| {
            let words = sentinel_order(line);
            match words.as_slice() {
                [sentinel] => *sentinel,
                [] if line.trim().is_empty() => "",
                _ => line,
            }
        })
        .collect();
    rows.truncate(rows.iter().position(|row| *row == "ZZZ").expect(text) + 1);
    rows
}

#[test]
fn relative_continuations_adopt_spacing_without_crossing_committed_siblings() {
    // Fixed CVS man_term.c::pre_RS/post_RS close the row and change the
    // offset in place. roff_term.c::roff_term_pre_sp executes every requested
    // row before the following scope. The exact inputs ran through CVS.
    let cases: &[(&str, &[&str], bool)] = &[
        ("", &["AAA", "BBB", "DDD", "", "ZZZ"], true),
        ("CCC\n", &["AAA", "BBB", "CCC", "DDD", "", "ZZZ"], false),
        (".B CCC\n", &["AAA", "BBB", "CCC", "DDD", "", "ZZZ"], false),
        (
            ".UR https://example.test/\nCCC\n.UE\n",
            &["AAA", "BBB", "CCC", "DDD", "", "ZZZ"],
            false,
        ),
        (
            ".nf\nCCC\n.fi\n",
            &["AAA", "BBB", "CCC", "DDD", "", "ZZZ"],
            false,
        ),
        (".PP\n", &["AAA", "BBB", "", "DDD", "", "ZZZ"], false),
        (".sp 0\n", &["AAA", "BBB", "DDD", "", "ZZZ"], true),
        (".sp 1\n", &["AAA", "BBB", "", "DDD", "", "ZZZ"], true),
        (
            ".sp 3\n",
            &["AAA", "BBB", "", "", "", "DDD", "", "ZZZ"],
            true,
        ),
        (
            ".sp 1\nCCC\n.sp 3\n",
            &["AAA", "BBB", "", "CCC", "", "", "", "DDD", "", "ZZZ"],
            false,
        ),
        (
            ".sp 3\n.RS 5n\n.RE\n",
            &["AAA", "BBB", "", "", "", "DDD", "", "ZZZ"],
            true,
        ),
        (
            ".nf\n.sp 3\n.fi\n",
            &["AAA", "BBB", "", "", "", "DDD", "", "ZZZ"],
            true,
        ),
        (
            ".sp 1\n.sp 2\n",
            &["AAA", "BBB", "", "", "", "DDD", "", "ZZZ"],
            true,
        ),
    ];
    for first_tag in [".TP\n1.", ".IP 1.", ".TP\n(2)", ".TP\nname"] {
        for &(between, expected_rows, continues) in cases {
            let source = roff_ordered_continuation(first_tag, between);
            let parsed = load_roff_bytes(source.as_bytes()).unwrap();
            let content = transport(&parsed);
            let expected_order: Vec<_> = expected_rows
                .iter()
                .copied()
                .filter(|word| !word.is_empty())
                .collect();
            let text = render_query_text(&content);
            assert_eq!(sentinel_order(&text), expected_order, "{source}\n{text}");
            assert_eq!(sentinel_rows(&text), expected_rows, "{source}\n{text}");
            let markdown = mant_codec::encode::render_markdown(&content);
            let readback = load_markdown_text(&markdown, None).unwrap();
            assert_eq!(
                sentinel_order(&render_query_text(&readback)),
                expected_order,
                "{source}\n{markdown}"
            );

            assert_continuation_owner(&content, first_tag, continues, expected_rows, &source);
        }
    }
}

fn assert_continuation_owner(
    content: &ResolvedContent,
    first_tag: &str,
    continues: bool,
    expected_rows: &[&str],
    source: &str,
) {
    let blocks = &content.document.as_ref().unwrap().sections[0].blocks;
    if first_tag == ".TP\nname" {
        assert!(matches!(blocks.first(), Some(Block::DefinitionList { .. })));
    } else {
        let Some(Block::List { items, .. }) = blocks.first() else {
            panic!("expected ordered list: {source}");
        };
        assert_eq!(items.len(), 1, "{source}");
        assert!(
            paragraph_layout(&items[0].blocks, "BBB").is_some(),
            "{source}"
        );
        assert_eq!(
            paragraph_layout(&items[0].blocks, "DDD").is_some(),
            continues,
            "{source}"
        );
        if continues {
            let blank_lines = items[0]
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::VerticalSpace { lines, .. } => Some(usize::from(*lines)),
                    _ => None,
                })
                .sum::<usize>();
            assert_eq!(
                blank_lines,
                expected_rows[2..]
                    .iter()
                    .take_while(|row| row.is_empty())
                    .count(),
                "{source}"
            );
        }
    }
}

#[test]
fn consecutive_ordinal_siblings_each_keep_their_own_relative_scope() {
    for (first, next, start) in [
        (".TP\n1.", ".TP\n2.", 1),
        (".IP 1.", ".IP 2.", 1),
        (".TP\n(2)", ".TP\n(3)", 2),
    ] {
        let source = roff_ordered_continuation(first, &format!("{next}\nCCC\n"));
        let content = transport(&load_roff_bytes(source.as_bytes()).unwrap());
        let blocks = &content.document.as_ref().unwrap().sections[0].blocks;
        let Some(Block::List {
            kind: ListKind::Ordered {
                start: Some(actual_start),
            },
            items,
            ..
        }) = blocks.first()
        else {
            panic!("expected ordered list: {source}");
        };
        assert_eq!(*actual_start, start);
        assert_eq!(items.len(), 2, "{source}");
        assert!(
            paragraph_layout(&items[0].blocks, "BBB").is_some(),
            "{source}"
        );
        assert!(
            paragraph_layout(&items[0].blocks, "DDD").is_none(),
            "{source}"
        );
        assert!(
            paragraph_layout(&items[1].blocks, "BBB").is_none(),
            "{source}"
        );
        assert!(
            paragraph_layout(&items[1].blocks, "DDD").is_some(),
            "{source}"
        );
        let text = render_query_text(&content);
        assert_eq!(
            sentinel_rows(&text),
            ["AAA", "BBB", "", "CCC", "DDD", "", "ZZZ"]
        );
    }
}

#[test]
fn illumos_getopts_keeps_every_flag_before_its_description() {
    // The complete fixture's CVS UTF-8/ASCII rows keep each label ahead of
    // the description in its following native RS scope.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/ordered-continuation/illumos-getopts.1");
    let content = transport(&load_roff_bytes(&std::fs::read(fixture).unwrap()).unwrap());
    let markdown = mant_codec::encode::render_markdown(&content);
    let readback = load_markdown_text(&markdown, None).unwrap();
    let pairs = [
        ("c", "Cache this optstring for multiple passes."),
        ("i", "Ignore this optstring when generating help."),
        ("l", "Display only long option names in help messages."),
        ("o", "The - option character prefix is optional."),
        ("p", "The number specifies the number of - characters"),
        ("s", "The number specifies the manual page section number,"),
    ];
    for representation in [&content, &readback] {
        let text = render_query_text(representation);
        let mut rows = text
            .lines()
            .skip_while(|row| !row.contains("The flags are:"))
            .skip(1)
            .map(str::trim)
            .filter(|row| !row.is_empty())
            .peekable();
        for (label, description) in pairs {
            assert_eq!(rows.next(), Some(label), "{description}");
            let row = rows.next().unwrap();
            assert!(row.starts_with(description), "{label}: {row}");
            assert_eq!(text.matches(description).count(), 1, "{label}");
            if label != "s" {
                while rows.next_if(|row| row.len() != 1).is_some() {}
            }
        }
    }
}
