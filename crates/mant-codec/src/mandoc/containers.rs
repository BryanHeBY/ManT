//! Streaming container boundaries shared by inline and structural consumers.
//!
//! Container transparency means that payloads remain reachable. It does not
//! mean that the entire container may be skipped by logical sibling lookup.
use libmandoc_rs::{Node, NodeKind};

use super::{first_part_children, inline::enclosure_marks, roff_escape::RoffFont};

pub(super) enum Event<'a> {
    /// An executed wrapper boundary whose children are emitted separately.
    BeginNode(&'a Node),
    /// A formatter flush boundary, distinct from an extra blank row.
    Break,
    /// Emit the current output row even if preceding requests emptied it.
    FlushLine,
    Children(&'a [Node]),
    Glyph(String),
    Tight,
    Release,
    EmptyWord,
    EnterKeep,
    ExitKeep,
    EnterFont(RoffFont),
    ExitFont,
}

pub(super) fn is_container(node: &Node) -> bool {
    matches!(node.macro_name.as_deref(), Some("Bf" | "Bk" | "ce" | "rj"))
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
        Some("Bk") => {
            emit(Event::EnterKeep);
            emit(Event::Children(body));
            emit(Event::ExitKeep);
        }
        Some("ce" | "rj") => aligned_line_payload(node, &mut emit),
        Some("Eo") => authored_enclosure(node, &mut emit),
        name if super::inline::is_enclosure_macro(name) => {
            let marks = node.enclosure.as_ref().map_or_else(
                || {
                    if name == Some("En") {
                        Some((None, None))
                    } else {
                        enclosure_marks(name.unwrap_or_default())
                            .map(|(a, b)| (Some(a.to_owned()), Some(b.to_owned())))
                    }
                },
                |enclosure| {
                    Some((
                        Some(super::roff_escape::visible_text(&enclosure.opening)),
                        enclosure
                            .closing
                            .as_deref()
                            .map(super::roff_escape::visible_text),
                    ))
                },
            );
            let Some((open, close)) = marks else {
                return false;
            };
            if let Some(open) = open {
                if open.is_empty() {
                    emit(Event::EmptyWord);
                } else {
                    emit(Event::Glyph(open));
                }
                emit(Event::Tight);
            }
            let children = node
                .children
                .iter()
                .find(|child| child.kind == NodeKind::Body)
                .map_or(node.children.as_slice(), |body| body.children.as_slice());
            emit(Event::Children(children));
            if let Some(close) = close {
                emit(Event::Tight);
                if close.is_empty() {
                    emit(Event::EmptyWord);
                } else {
                    emit(Event::Glyph(close));
                }
            } else {
                // An absent obsolete `.Es` closing delimiter emits no word,
                // but mdoc_term.c still releases TERMP_NOSPACE after `.En`.
                emit(Event::Release);
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

/// Native `ce`/`rj` own a control count followed by actual input lines and
/// intervening requests. Mirror `roff_term_pre_ce`'s child grouping, without
/// implementing device-specific centering/right alignment. Neither numeric
/// body text nor state/spacing requests belong to the discarded count slot.
fn aligned_line_payload<'a>(node: &'a Node, emit: &mut impl FnMut(Event<'a>)) {
    emit(Event::Break);
    let children = node.children.get(1..).unwrap_or_default();
    let mut start = 0;
    while start < children.len() {
        let end = children[start + 1..]
            .iter()
            .position(|child| child.kind == NodeKind::Text && child.flags.line_start)
            .map_or(children.len(), |relative| start + 1 + relative);
        let mut pending = start;
        for index in start..end {
            // roff_term_pre_ce executes these through roff_term_pre_br
            // (ti calls it before applying its omitted device geometry),
            // irrespective of NODE_NOFILL. An inline hard-break marker is
            // not enough: the group-end flush must see the emptied row.
            // Still dispatch the original request afterwards, preserving
            // fill-state transitions and any source/target handling.
            if matches!(
                children[index].macro_name.as_deref(),
                Some("br" | "fi" | "nf" | "ti")
            ) {
                if pending < index {
                    emit(Event::Children(&children[pending..index]));
                }
                emit(Event::Break);
                pending = index;
            }
        }
        emit(Event::Children(&children[pending..end]));
        emit(Event::FlushLine);
        start = end;
    }
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
        || matches!(node.macro_name.as_deref(), Some("ce" | "rj"))
        || (node.kind == NodeKind::Block
            && matches!(
                node.macro_name.as_deref(),
                Some("Bl" | "Rs" | "Bd" | "D1" | "Dl")
            ))
        || node.children.iter().any(has_structural_payload)
}
