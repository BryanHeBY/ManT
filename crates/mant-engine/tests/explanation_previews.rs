//! Match windows carry real final-IR locations and Unicode scalar ranges.
use mant_engine::{explain_query, query_markdown_text, resolve_explanation_block};
use mant_protocol::{EvidenceClass, ExplanationOptions, ExplanationQuery};

#[test]
fn previews_keep_two_original_blocks_complete_matches_and_atomic_body() {
    let needle = "界".repeat(512);
    let long = format!("{} {needle} {}", "前".repeat(1500), "後".repeat(1500));
    let source = format!(
        "# Tool\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `-Q`: {long}\n\n  Second {needle} match.\n\n  Third {needle} match.\n\n  No match, unrelated full body.\n"
    );
    let content = query_markdown_text(&source, None).unwrap();
    let mut query = ExplanationQuery {
        entry: needle.clone(),
        options: ExplanationOptions::default(),
    };
    let result = explain_query(&content, &query).unwrap();
    assert_eq!(result.total, 1);
    let e = &result.evidence[0];
    assert_eq!(e.class, EvidenceClass::EntryMention);
    assert_eq!(e.previews.len(), 2);
    assert!(!e.previews_omitted);
    assert!(e.previews[0].clipped_before && e.previews[0].clipped_after);
    assert!(!e.previews[1].clipped_before && !e.previews[1].clipped_after);
    assert!(e.previews[0].source.unwrap().line < e.previews[1].source.unwrap().line);
    for preview in &e.previews {
        assert!(preview.text.chars().count() <= 1024);
        assert_eq!(
            preview
                .text
                .chars()
                .skip(preview.match_start_char as usize)
                .take((preview.match_end_char - preview.match_start_char) as usize)
                .collect::<String>(),
            needle
        );
        let block =
            resolve_explanation_block(content.document.as_ref().unwrap(), &preview.block_path)
                .unwrap();
        assert!(
            matches!(block, mant_ir::Block::Paragraph { source, .. } if *source == preview.source)
        );
    }
    let rendered = mant_engine::render_explanation_text(&result);
    assert!(!rendered.contains("No match, unrelated full body"));
    assert!(
        serde_json::to_string(e.content.as_ref().unwrap())
            .unwrap()
            .contains("unrelated full body")
    );
    // Details + windows fit; complete large body does not. Windows are not
    // placed into content and window clipping is not a budget truncation.
    query.options.content_bytes = 6000;
    let small = explain_query(&content, &query).unwrap();
    let e = &small.evidence[0];
    assert_eq!(e.previews.len(), 2);
    assert!(!e.previews_omitted);
    assert!(e.content.is_none() && e.content_omitted);
    assert!(small.truncation.content);
}

#[test]
fn safe_projection_coordinates_resolve_nested_items_and_cells() {
    let content = query_markdown_text("# Tool\n\n## Outer\n\n### Inner\n\n- Plain\n\n  - Nested TOKEN.\n\n| Title |\n| --- |\n| TOKEN |\n\n```\ncontrol\u{1b} TOKEN\n```\n", None).unwrap();
    let result = explain_query(
        &content,
        &ExplanationQuery {
            entry: "TOKEN".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.total, 3);
    let document = content.document.as_ref().unwrap();
    for e in &result.evidence {
        let preview = &e.previews[0];
        assert!(resolve_explanation_block(document, &preview.block_path).is_some());
        let range = preview.match_start_char as usize..preview.match_end_char as usize;
        assert_eq!(
            preview
                .text
                .chars()
                .skip(range.start)
                .take(range.len())
                .collect::<String>(),
            "TOKEN"
        );
        assert!(!preview.text.contains('\u{1b}'));
    }
    assert!(
        result
            .evidence
            .iter()
            .any(|e| e.previews[0].block_path.contains("/r1/c0/"))
    );
    assert!(
        result
            .evidence
            .iter()
            .any(|e| e.previews[0].block_path.contains("/i0/"))
    );
    for bad in [
        "sections",
        "sections/s99/b0",
        "root/b-1",
        "root/b0/",
        "root/b0/r0",
        "sections/s0/s0/b99999999999999999999999",
    ] {
        assert!(resolve_explanation_block(document, bad).is_none());
    }
}

#[test]
fn markdown_metadata_is_escaped_and_mentions_are_not_definitions() {
    let content = query_markdown_text("# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `-Q`: The literal `[TOKEN](evil)` occurs here.\n\n  PRIVATE_UNRELATED_TEXT\n", None).unwrap();
    let found = explain_query(
        &content,
        &ExplanationQuery {
            entry: "TOKEN".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(found.counts.direct_entry.total, 0);
    let markdown = mant_engine::render_explanation_markdown(&found);
    assert!(markdown.contains("\\[**TOKEN**\\]\\(evil\\)"), "{markdown}");
    assert!(!markdown.contains("PRIVATE_UNRELATED_TEXT"));
    assert!(!markdown.contains("## Direct entries"));
    assert!(markdown.contains("not proof that an option is absent"));
}
