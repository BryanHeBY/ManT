use mant_ir::{Block, Inline, SourceSpan};

use super::{BlockState, layout};
use crate::mandoc::inline::plain_text;

fn text(value: &str) -> Vec<Inline> {
    vec![Inline::Text {
        value: value.to_owned(),
    }]
}

const fn source(line: u32) -> SourceSpan {
    SourceSpan {
        byte_range: None,
        line,
        column: 1,
        end_line: None,
        end_column: None,
    }
}

#[test]
fn block_state_preserves_filled_line_boundaries_and_continuations() {
    let mut state = BlockState::with_output(3.into(), true, Vec::new());
    state.push_inline(text("alpha"), Some(source(1)), false, false);
    state.push_inline(text("beta"), Some(source(2)), false, false);
    state.push_inline(text("gamma"), Some(source(3)), true, false);
    state.push_inline(text("delta"), Some(source(4)), false, true);
    state.push_inline(text("epsilon"), Some(source(5)), false, false);

    let output = state.finish();
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
    assert_eq!(plain_text(children), "alpha beta\ngamma deltaepsilon");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        1
    );
    assert_eq!(*paragraph_layout, layout(3.into()));
    assert_eq!(*paragraph_source, Some(source(1)));
}

#[test]
fn block_state_flushes_paragraph_before_tight_preformatted_lines() {
    let mut state = BlockState::with_output(2.into(), true, Vec::new());
    state.push_inline(text("prose"), Some(source(1)), false, false);
    state.push_preformatted(text("first"), Some(source(3)), false, true, true);
    state.push_preformatted(text("second"), Some(source(4)), true, true, true);
    state.push_preformatted(text("third"), Some(source(5)), false, true, true);

    let output = state.finish();
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
    assert_eq!(plain_text(paragraph), "prose");
    assert_eq!(plain_text(preformatted), "first\nsecondthird");
    assert_eq!(
        preformatted
            .iter()
            .filter(|inline| matches!(inline, Inline::LineBreak))
            .count(),
        1
    );
    assert_eq!(*paragraph_layout, layout(2.into()));
    assert_eq!(*preformatted_layout, layout(2.into()));
    assert_eq!(*paragraph_source, Some(source(1)));
    assert_eq!(*preformatted_source, Some(source(3)));
    assert_eq!(*language, None);
}
