//! Place typed link annotations around their accepted source-owned output.

use super::{Inline, InlineBuilder};
use mant_ir::{LinkTarget, email_address_from_mailto_uri};

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
        .new_annotation(source_link_target(address.to_owned(), email));
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

pub(super) fn source_link_target(address: String, email: bool) -> LinkTarget {
    if email {
        LinkTarget::Email { address }
    } else if let Some(address) = email_address_from_mailto_uri(&address) {
        LinkTarget::Email { address }
    } else {
        LinkTarget::External { uri: address }
    }
}
