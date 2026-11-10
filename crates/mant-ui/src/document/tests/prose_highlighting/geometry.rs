use super::*;

#[test]
fn every_utf8_inline_split_retains_the_same_template_colors_and_text() {
    let value = "文件e\u{301}👩‍💻";
    let baseline = content(vec![
        strong("-fplugin-arg-"),
        emphasis(value),
        strong("=arg -c -S"),
    ]);
    let expected = DocumentView::new(&baseline).render(120);
    for split in value
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(value.len()))
    {
        let actual = content(vec![
            strong("-fplugin-arg-"),
            Inline::Emphasis {
                children: vec![
                    text(&value[..split]),
                    Inline::anchor("split"),
                    text(&value[split..]),
                ],
            },
            strong("=arg -c -S"),
        ]);
        let rendered = DocumentView::new(&actual).render(120);
        assert_eq!(rendered.text, expected.text, "split {split}");
        assert_eq!(copied_match(&rendered, value), value);
        assert!(rendered.anchor_row("split").is_some());
        assert_eq!(style_at(&rendered, value).fg, Some(theme::SAPPHIRE));
    }
}

#[test]
fn linked_templates_keep_cell_targets_copy_and_search_across_width_changes() {
    let value = "文件e\u{301}👩‍💻";
    let original = content(vec![
        strong("-c -S -fplugin-arg-"),
        Inline::Link {
            target: mant_ir::LinkTarget::Section {
                id: "description".into(),
            },
            title: None,
            children: vec![emphasis(value)],
        },
        strong("="),
        emphasis("argument"),
        Inline::anchor("argument-end"),
    ]);
    let before = original.clone();
    let encoded = serde_json::to_string(original.document.as_ref().unwrap()).unwrap();
    let mut restored = original.clone();
    restored.document = Some(serde_json::from_str(&encoded).unwrap());
    let view = DocumentView::new(&restored);
    for width in [12, 24, 120, 12] {
        let rendered = view.render(width);
        let hit = &rendered.search(value)[0];
        assert_eq!(
            rendered.link_target_at(hit.row, hit.start_column),
            Some(&LinkTarget::Section("description".into()))
        );
        let style = style_at(&rendered, value);
        assert_eq!(style.fg, Some(theme::LINK));
        assert!(
            style
                .add_modifier
                .contains(Modifier::ITALIC | Modifier::UNDERLINED)
        );
        assert_eq!(
            copied_match(&rendered, value)
                .lines()
                .map(str::trim_start)
                .collect::<String>(),
            value
        );
        assert!(rendered.anchor_row("argument-end").is_some());
        assert_eq!(view.render(width).text, rendered.text);
    }
    assert_eq!(restored, before);
}

#[test]
fn excessive_body_owners_fall_back_without_partial_reference_coloring() {
    for source in [
        format!("--help {}", "x".repeat(crate::code::MAX_BLOCK_BYTES)),
        format!("--help {}FILE{}", "[".repeat(65), "]".repeat(65)),
    ] {
        let original = content(vec![strong(&source)]);
        let rendered = DocumentView::new(&original).render(120);
        assert_eq!(style_at(&rendered, "--help").fg, Some(theme::STRONG));
    }
}
