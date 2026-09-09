//! Streaming container boundaries shared by inline and structural consumers.
//!
//! Container transparency means that payloads remain reachable. It does not
//! mean that the entire container may be skipped by logical sibling lookup.
use libmandoc_rs::{Node, NodeKind};

use super::{first_part_children, inline::enclosure_marks, roff_escape::RoffFont};

pub(super) enum Event<'a> {
    /// An executed wrapper boundary whose children are emitted separately.
    BeginNode(&'a Node),
    Children(&'a [Node]),
    Glyph(String),
    Tight,
    Release,
    EmptyWord,
    EnterFont(RoffFont),
    ExitFont,
}

pub(super) fn is_container(node: &Node) -> bool {
    matches!(node.macro_name.as_deref(), Some("Bf" | "Bk"))
        || super::inline::is_enclosure_macro(node.macro_name.as_deref())
        || (node.macro_name.is_none()
            && matches!(
                node.kind,
                NodeKind::Root | NodeKind::Head | NodeKind::Body | NodeKind::Tail
            ))
}

/// Emit a bounded sequence of boundaries and borrowed child slices. Children
/// are consumed immediately; there is no document-sized intermediate stream.
pub(super) fn walk(node: &Node, mut emit: impl FnMut(Event<'_>)) -> bool {
    let body = first_part_children(node, NodeKind::Body);
    match node.macro_name.as_deref() {
        Some("Bf") => {
            if let Some(font) = node.font {
                emit(Event::EnterFont(font.into()));
            }
            emit(Event::Children(body));
            if node.font.is_some() {
                emit(Event::ExitFont);
            }
        }
        Some("Bk") => emit(Event::Children(body)),
        Some("Eo") => authored_enclosure(node, &mut emit),
        name if super::inline::is_enclosure_macro(name) => {
            let marks = node.enclosure.as_ref().map_or_else(
                || {
                    enclosure_marks(name.unwrap_or_default())
                        .map(|(a, b)| (a.to_owned(), b.to_owned()))
                },
                |enclosure| {
                    Some((
                        super::roff_escape::visible_text(&enclosure.opening),
                        enclosure
                            .closing
                            .as_deref()
                            .map(super::roff_escape::visible_text)
                            .unwrap_or_default(),
                    ))
                },
            );
            let Some((open, close)) = marks else {
                return false;
            };
            if !open.is_empty() {
                emit(Event::Glyph(open));
                emit(Event::Tight);
            }
            let children = node
                .children
                .iter()
                .find(|child| child.kind == NodeKind::Body)
                .map_or(node.children.as_slice(), |body| body.children.as_slice());
            emit(Event::Children(children));
            if !close.is_empty() {
                emit(Event::Tight);
                emit(Event::Glyph(close));
            }
            if node
                .children
                .iter()
                .any(|child| child.kind == NodeKind::Body)
            {
                for child in node
                    .children
                    .iter()
                    .filter(|child| child.flags.delimiter_close)
                {
                    emit(Event::Children(std::slice::from_ref(child)));
                }
            }
        }
        None if matches!(
            node.kind,
            NodeKind::Root | NodeKind::Head | NodeKind::Body | NodeKind::Tail
        ) =>
        {
            emit(Event::Children(&node.children));
        }
        _ => return false,
    }
    true
}

/// Authored delimiters can be siblings of the body wrappers. Keep wrapper
/// execution events in that same stream even when an Ec tail has no text.
fn authored_enclosure<'a>(node: &'a Node, emit: &mut impl FnMut(Event<'a>)) {
    let head = first_part_children(node, NodeKind::Head);
    let body = first_part_children(node, NodeKind::Body);
    let tail = first_part_children(node, NodeKind::Tail);
    for (index, child) in node.children.iter().enumerate() {
        if matches!(child.kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail) {
            emit(Event::BeginNode(child));
        }
        match child.kind {
            NodeKind::Head => {
                emit(Event::Children(&child.children));
                if !head.is_empty() && (!body.is_empty() || !tail.is_empty()) {
                    emit(Event::Tight);
                }
            }
            NodeKind::Body => {
                emit(Event::Children(&child.children));
                if head.is_empty() && body.is_empty() && tail.is_empty() {
                    emit(Event::EmptyWord);
                } else if tail.is_empty() {
                    emit(Event::Release);
                }
            }
            NodeKind::Tail => {
                if !tail.is_empty() && (!head.is_empty() || !body.is_empty()) {
                    emit(Event::Tight);
                }
                emit(Event::Children(&child.children));
            }
            _ => emit(Event::Children(&node.children[index..=index])),
        }
    }
}

/// A payload consumer must never flatten these nodes through inline children.
pub(super) fn has_structural_payload(node: &Node) -> bool {
    node.kind == NodeKind::Table
        || (node.kind == NodeKind::Block
            && matches!(
                node.macro_name.as_deref(),
                Some("Bl" | "Rs" | "Bd" | "D1" | "Dl")
            ))
        || node.children.iter().any(has_structural_payload)
}
