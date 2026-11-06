//! Place typed link annotations around their accepted source-owned output.

use super::{Inline, InlineBuilder};

/// Wrap newly executed visible content without interrupting the caller's
/// formatter state. A link is an IR annotation around output, not an atomic
/// source fragment: following `Ns`, `\\z`, font, and word-boundary events still
/// observe the same builder state.
pub(super) fn append_external_link(
    builder: &mut InlineBuilder,
    address: &str,
    email: bool,
    append: impl FnOnce(&mut InlineBuilder),
) {
    let registered = builder
        .zero_advance
        .new_annotation(external_link_target(address.to_owned(), email));
    let target = registered.target;
    let owner = registered.key;
    let (_, membership) = builder.append_semantic_scope(owner, true, append, |children| {
        vec![Inline::Link {
            target: target.clone(),
            title: None,
            children,
        }]
    });
    builder.zero_advance.bind_deferred(
        membership,
        &crate::mandoc::inline::ownership::Annotation::Link {
            target,
            title: None,
        },
        true,
    );
}

pub(super) fn external_link_target(address: String, email: bool) -> mant_ir::LinkTarget {
    if email {
        mant_ir::LinkTarget::Email { address }
    } else {
        mant_ir::LinkTarget::External { uri: address }
    }
}
