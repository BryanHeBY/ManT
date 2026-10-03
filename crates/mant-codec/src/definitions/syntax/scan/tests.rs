//! Source-neutral declaration grammar controls; no formatter gold is inferred.

use super::*;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}
fn strong(nodes: Vec<Inline>) -> Inline {
    Inline::Strong { children: nodes }
}
fn emphasis(nodes: Vec<Inline>) -> Inline {
    Inline::Emphasis { children: nodes }
}

fn assert_names(nodes: &[Inline], operands: &[NativeOperand], expected: &[&str]) {
    let result = option_head(nodes, operands);
    assert_eq!(
        result
            .names
            .iter()
            .map(|name| name.name.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    let visible = mant_ir::inline_plain_text(nodes);
    for name in result.names {
        assert_eq!(name.parts.len(), 1);
        assert_eq!(&visible[name.parts[0].clone()], name.name);
    }
}

mod declarations;
mod limits;
mod native_operands;
mod parameters;
mod wrappers;
