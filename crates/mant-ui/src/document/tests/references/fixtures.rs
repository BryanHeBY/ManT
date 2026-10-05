//! Shared document targets and labelled body roots.
use super::super::{Block, Inline, LayoutHint};

pub(super) fn document_link(label: &str, fragment: Option<&str>) -> Inline {
    Inline::Link {
        target: mant_ir::LinkTarget::Document {
            name: "target".into(),
            fragment: fragment.map(str::to_owned),
        },
        title: None,
        children: vec![Inline::Text {
            value: label.into(),
        }],
    }
}

pub(super) fn linked_block(prefix: &str, label: &str) -> Block {
    Block::Paragraph {
        inline_layout: mant_ir::InlineLayout::default(),
        children: vec![
            Inline::Text {
                value: prefix.into(),
            },
            document_link(label, None),
        ],
        layout: LayoutHint::default(),
        source: None,
    }
}
