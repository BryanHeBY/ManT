use mant_ir::{Block, Inline, SourceSpan};

use super::{BlockState, layout};
use crate::mandoc::{content::LegacyContent, inline::DraftInline};

fn text(value: &str) -> Vec<DraftInline> {
    vec![DraftInline::Text {
        value: value.to_owned(),
    }]
}

fn inline_text(content: mant_ir::ContentContext<'_>, children: &[Inline]) -> String {
    mant_ir::inline_plain_text(content, children)
}

const fn source(line: u32) -> SourceSpan {
    SourceSpan {
        source: mant_ir::SourceKey::FIRST,
        byte_range: None,
        line,
        column: 1,
        end_line: None,
        end_column: None,
    }
}

#[test]
fn block_state_preserves_filled_line_boundaries_and_continuations() {
    let content = LegacyContent::default();
    let mut state = BlockState::with_output(3.into(), true, Vec::new(), content.clone());
    state.push_inline(text("alpha"), Some(source(1)), false, false);
    state.push_inline(text("beta"), Some(source(2)), false, false);
    state.push_inline(text("gamma"), Some(source(3)), true, false);
    state.push_inline(text("delta"), Some(source(4)), false, true);
    state.push_inline(text("epsilon"), Some(source(5)), false, false);

    let output = state.finish();
    let store = content.finish();
    let content = store.content();
    let [
        Block::Paragraph {
            children,
            layout: paragraph_layout,
            source: paragraph_source,
        },
    ] = output.as_slice()
    else {
        panic!("expected one filled paragraph, got {output:?}");
    };
    assert_eq!(
        inline_text(content, children),
        "alpha beta\ngamma deltaepsilon"
    );
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak { .. }))
            .count(),
        1
    );
    assert_eq!(*paragraph_layout, layout(3.into()));
    assert_eq!(*paragraph_source, Some(source(1)));
}

#[test]
fn block_state_flushes_paragraph_before_tight_preformatted_lines() {
    let content = LegacyContent::default();
    let mut state = BlockState::with_output(2.into(), true, Vec::new(), content.clone());
    state.push_inline(text("prose"), Some(source(1)), false, false);
    state.push_preformatted(text("first"), Some(source(3)), false, true, true);
    state.push_preformatted(text("second"), Some(source(4)), true, true, true);
    state.push_preformatted(text("third"), Some(source(5)), false, true, true);

    let output = state.finish();
    let store = content.finish();
    let content = store.content();
    let [
        Block::Paragraph {
            children: paragraph,
            layout: paragraph_layout,
            source: paragraph_source,
        },
        Block::Preformatted {
            children: preformatted,
            language,
            layout: preformatted_layout,
            source: preformatted_source,
        },
    ] = output.as_slice()
    else {
        panic!("expected prose followed by one preformatted block, got {output:?}");
    };
    assert_eq!(inline_text(content, paragraph), "prose");
    assert_eq!(inline_text(content, preformatted), "first\nsecondthird");
    assert_eq!(
        preformatted
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak { .. }))
            .count(),
        1
    );
    assert_eq!(*paragraph_layout, layout(2.into()));
    assert_eq!(*preformatted_layout, layout(2.into()));
    assert_eq!(*paragraph_source, Some(source(1)));
    assert_eq!(*preformatted_source, Some(source(3)));
    assert_eq!(*language, None);
}

#[test]
fn block_state_finish_keeps_target_only_tail_and_original_source() {
    let content = LegacyContent::default();
    let mut state = BlockState::with_output(3.into(), true, Vec::new(), content.clone());
    state.queue_targets(["tail-target".to_owned()], Some(source(9)));
    state.flush_paragraph();
    state.flush_preformatted();
    let blocks = state.finish();
    let has_flushed_row = super::has_flushed_row(&content, &blocks);
    let store = content.finish();
    let content = store.content();
    let [
        Block::Paragraph {
            children,
            source: owner,
            ..
        },
    ] = blocks.as_slice()
    else {
        panic!("target-only tail remains addressable: {blocks:?}");
    };
    assert!(
        matches!(children.as_slice(), [Inline::Anchor { id, point, .. }]
            if id == "tail-target"
                && matches!(content.point(*point).map(|point| point.provenance), Some(mant_ir::Provenance::Authored { span }) if span.line == 9))
    );
    assert_eq!(*owner, Some(source(9)));
    assert!(!has_flushed_row);
}

#[test]
fn block_state_literal_take_resets_continuation_occupancy_and_provenance() {
    let content = LegacyContent::default();
    let mut state = BlockState::with_output(2.into(), true, Vec::new(), content.clone());
    state.push_preformatted(text("first"), Some(source(1)), true, true, true);
    state.flush_preformatted();
    state.flush_preformatted();
    state.queue_targets(["second".to_owned()], Some(source(3)));
    state.push_preformatted(Vec::new(), Some(source(3)), false, true, true);
    state.push_preformatted(text("second"), Some(source(4)), false, true, true);
    let blocks = state.finish();
    let store = content.finish();
    let content = store.content();
    let [
        Block::Preformatted {
            children: first,
            source: first_source,
            ..
        },
        Block::Preformatted {
            children: second,
            source: second_source,
            ..
        },
    ] = blocks.as_slice()
    else {
        panic!("two independent literal buffers: {blocks:?}");
    };
    assert_eq!(inline_text(content, first), "first");
    assert_eq!(inline_text(content, second), "\nsecond");
    assert_eq!(*first_source, Some(source(1)));
    assert_eq!(*second_source, Some(source(3)));
    assert!(matches!(second.first(), Some(Inline::Anchor { id, .. }) if id == "second"));
}

#[test]
fn block_state_paragraph_take_resets_source_cursor_and_pending_join() {
    let content = LegacyContent::default();
    let mut state = BlockState::with_output(0.into(), true, Vec::new(), content.clone());
    state.push_inline(text("first"), Some(source(1)), false, true);
    state.flush_paragraph();
    assert!(state.paragraph_is_empty());
    state.push_inline(text("second"), Some(source(4)), true, false);
    state.push_inline(text("third"), Some(source(5)), false, false);
    let blocks = state.finish();
    let store = content.finish();
    let content = store.content();
    let [
        Block::Paragraph {
            children: first, ..
        },
        Block::Paragraph {
            children: second,
            source: owner,
            ..
        },
    ] = blocks.as_slice()
    else {
        panic!("two paragraphs: {blocks:?}");
    };
    assert_eq!(inline_text(content, first), "first");
    assert_eq!(inline_text(content, second), "second third");
    assert_eq!(*owner, Some(source(4)));
}

#[test]
fn block_state_hp_flushes_first_literal_row_once_and_keeps_targets() {
    let content = LegacyContent::default();
    let mut state = BlockState::with_output(0.into(), true, Vec::new(), content.clone());
    state.start_hanging(8.into());
    state.queue_targets(["first".to_owned()], Some(source(1)));
    state.push_preformatted(text("one"), Some(source(1)), true, true, true);
    state.push_preformatted(text("continued"), Some(source(2)), false, true, true);
    state.push_preformatted(text("body"), Some(source(3)), false, true, true);
    state.literal_mode_boundary();
    let blocks = state.finish();
    let store = content.finish();
    let content = store.content();
    let [
        Block::Preformatted {
            children: first,
            layout: first_layout,
            ..
        },
        Block::Preformatted {
            children: body,
            layout: body_layout,
            ..
        },
    ] = blocks.as_slice()
    else {
        panic!("HP first/body lines: {blocks:?}");
    };
    assert_eq!(inline_text(content, first), "onecontinued");
    assert_eq!(inline_text(content, body), "body");
    assert_eq!(first_layout.indent_columns, 0);
    assert_eq!(body_layout.indent_columns, 8);
    assert!(matches!(first.first(), Some(Inline::Anchor { id, .. }) if id == "first"));
}
