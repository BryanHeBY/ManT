//! Codec-internal lowering contracts; no query or rendering dependencies.
use super::*;

mod columns;
mod equations;
mod fonts;
mod layout;
mod placement;
mod recovery;

fn contains_manual_link(children: &[Inline]) -> bool {
    children.iter().any(|inline| match inline {
        Inline::Link {
            target: mant_ir::LinkTarget::Manual { .. },
            ..
        } => true,
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => contains_manual_link(children),
        Inline::Text { .. }
        | Inline::Code { .. }
        | Inline::Equation { .. }
        | Inline::Anchor { .. }
        | Inline::LineBreak { .. } => false,
    })
}

fn contains_emphasis(children: &[Inline]) -> bool {
    children.iter().any(|inline| match inline {
        Inline::Emphasis { .. } => true,
        Inline::Strong { children } | Inline::Link { children, .. } => contains_emphasis(children),
        Inline::Text { .. }
        | Inline::Code { .. }
        | Inline::Equation { .. }
        | Inline::Anchor { .. }
        | Inline::LineBreak { .. } => false,
    })
}
