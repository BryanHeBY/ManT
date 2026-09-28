//! Streaming container boundaries shared by inline and structural consumers.
//!
//! Container transparency means that payloads remain reachable. It does not
//! mean that the entire container may be skipped by logical sibling lookup.
use libmandoc_rs::{Node, NodeKind};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use super::inline::FontScope;
use super::{first_part_children, inline::enclosure_marks, roff_escape::RoffFont};

pub(super) enum Event<'a> {
    /// Source owner of subsequent generated output. True executes this
    /// node's `NODE_LINE`; false only changes ownership for a later post.
    At(&'a Node, bool),
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
    EnterFont(RoffFont, Option<u32>),
    ExitFont(Option<u32>),
    /// A direct Fa child of a structural Fo body; the sibling relation is
    /// needed for mandoc's generated comma after an argument.
    FunctionArgument(&'a Node, bool),
}

/// Private execution record for mdoc BODY posts. Clones share one source-order
/// walk even when a list or display temporarily owns a separate output buffer.
#[derive(Clone, Default)]
pub(super) struct ScopePostState(Rc<RefCell<ScopePosts>>);

#[derive(Default)]
struct ScopePosts {
    ended: HashSet<u32>,
    function_suffix: HashMap<u32, bool>,
    structural_payload: HashSet<u32>,
    font_scopes: Vec<(u32, FontScope)>,
    indexed: bool,
}

impl ScopePostState {
    pub(super) fn index_structural_payload(&self, root: &Node) {
        fn visit(node: &Node, index: &mut HashSet<u32>) -> bool {
            let mut has = is_structural_payload(node);
            for child in &node.children {
                has |= visit(child, index);
            }
            if has {
                index.insert(node.id);
            }
            has
        }
        let mut posts = self.0.borrow_mut();
        posts.structural_payload.clear();
        visit(root, &mut posts.structural_payload);
        posts.indexed = true;
    }

    pub(super) fn has_structural_payload(&self, node: &Node) -> bool {
        let posts = self.0.borrow();
        if posts.indexed {
            posts.structural_payload.contains(&node.id)
        } else {
            // Standalone inline builders in focused tests do not have a
            // document context; their small borrowed subtree is scanned.
            has_structural_payload(node)
        }
    }

    pub(super) fn ended(&self, body_id: u32) -> bool {
        self.0.borrow().ended.contains(&body_id)
    }

    pub(super) fn finish(&self, body_id: u32) {
        self.0.borrow_mut().ended.insert(body_id);
    }

    pub(super) fn register_function(&self, body_id: u32, synopsis: bool) {
        self.0
            .borrow_mut()
            .function_suffix
            .insert(body_id, synopsis);
    }

    pub(super) fn function_suffix(&self, body_id: u32) -> Option<bool> {
        self.0.borrow().function_suffix.get(&body_id).copied()
    }

    pub(super) fn enter_font(&self, body_id: u32, saved: FontScope) {
        self.0.borrow_mut().font_scopes.push((body_id, saved));
    }

    pub(super) fn exit_font(&self, body_id: u32) -> Option<FontScope> {
        let mut posts = self.0.borrow_mut();
        let index = posts
            .font_scopes
            .iter()
            .rposition(|(id, _)| *id == body_id)?;
        let saved = posts.font_scopes[index].1;
        // mdoc_term.c::print_mdoc_node() pops to the original BODY's
        // prev_font, including any crossed scopes above that BODY.
        posts.font_scopes.truncate(index);
        Some(saved)
    }
}

pub(super) fn is_container(node: &Node) -> bool {
    matches!(
        node.macro_name.as_deref(),
        Some("Bf" | "Bk" | "Fo" | "ce" | "rj")
    ) || super::inline::is_enclosure_macro(node.macro_name.as_deref())
        || (node.macro_name.is_none()
            && matches!(
                node.kind,
                NodeKind::Root | NodeKind::Head | NodeKind::Body | NodeKind::Tail
            ))
}

/// Emit a bounded sequence of boundaries and borrowed child slices. Children
/// are consumed immediately; there is no document-sized intermediate stream.
pub(super) fn walk<'a>(
    node: &'a Node,
    posts: &ScopePostState,
    mut emit: impl FnMut(Event<'a>),
) -> bool {
    // DisplayFlow probes every node before structural dispatch. A rejected
    // node must emit nothing: even an ownership-only At event would make the
    // display callback attach its target before Bd is lowered for real.
    if !is_container(node) {
        return false;
    }
    if node.scope_end.is_none()
        && node.macro_name.as_deref() == Some("Fo")
        && !posts.has_structural_payload(node)
    {
        return false;
    }
    emit(Event::At(node, true));
    if walk_scope_end(node, posts, &mut emit) {
        return true;
    }
    let body = first_part_children(node, NodeKind::Body);
    match node.macro_name.as_deref() {
        Some("Bf") => {
            let body_id = node
                .children
                .iter()
                .find(|part| part.kind == NodeKind::Body)
                .map(|part| part.id);
            if let Some(font) = node.font {
                emit(Event::EnterFont(font.into(), body_id));
            }
            emit(Event::Children(body));
            if node.font.is_some() {
                emit(Event::ExitFont(body_id));
            }
        }
        Some("Bk") => {
            emit(Event::EnterKeep);
            emit(Event::Children(body));
            emit(Event::ExitKeep);
        }
        Some("Fo") if posts.has_structural_payload(node) => {
            emit_structural_function(node, posts, &mut emit);
        }
        Some("ce" | "rj") => aligned_line_payload(node, &mut emit),
        Some("Eo") => authored_enclosure(node, posts, &mut emit),
        name if super::inline::is_enclosure_macro(name) => {
            let marks = resolved_enclosure_marks(node);
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
            let body_id = node
                .children
                .iter()
                .find(|part| part.kind == NodeKind::Body && part.scope_end.is_none())
                .map(|body| body.id);
            if body_id.is_none_or(|id| !posts.ended(id)) {
                if let Some(body) = node
                    .children
                    .iter()
                    .find(|part| part.kind == NodeKind::Body && part.scope_end.is_none())
                {
                    emit(Event::At(body, false));
                }
                emit_enclosure_post(close.as_deref(), &mut emit);
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

fn walk_scope_end<'a>(
    node: &'a Node,
    posts: &ScopePostState,
    emit: &mut impl FnMut(Event<'a>),
) -> bool {
    // CVS mdoc_html.c::print_mdoc_node() executes an ENDBODY marker's post
    // at its source position and marks the original BODY ended. CVS
    // mdoc.c::mdoc_endbody_alloc() retains that BODY relation on the marker.
    let Some(end) = node.scope_end else {
        return false;
    };
    if node.macro_name.as_deref() == Some("Bf") {
        emit(Event::Children(&node.children));
        emit(Event::ExitFont(Some(end.body_id)));
        posts.finish(end.body_id);
        return true;
    }
    if node.macro_name.as_deref() == Some("Fo") {
        emit(Event::Children(&node.children));
        if let Some(synopsis) = posts.function_suffix(end.body_id) {
            emit(Event::Tight);
            emit(Event::Glyph(if synopsis { ");" } else { ")" }.to_owned()));
            posts.finish(end.body_id);
        }
        return true;
    }
    if node.macro_name.as_deref() == Some("Eo") {
        emit_eo_endbody(node, emit);
        posts.finish(end.body_id);
        return true;
    }
    if let Some((_, close)) = resolved_enclosure_marks(node) {
        if !node.children.is_empty() {
            emit(Event::BeginNode(node));
        }
        emit(Event::Children(&node.children));
        emit_enclosure_post(close.as_deref(), emit);
        posts.finish(end.body_id);
        return true;
    }
    false
}

fn emit_eo_endbody<'a>(node: &'a Node, emit: &mut impl FnMut(Event<'a>)) {
    if !node.children.is_empty() {
        emit(Event::BeginNode(node));
        // mdoc_html.c::mdoc_eo_pre() sets HTML_NOSPACE before a nonempty
        // body-end marker; mdoc_eo_post() releases it.
        emit(Event::Tight);
    }
    emit(Event::Children(&node.children));
    emit(Event::Release);
}

/// Keep the native Fo HEAD/BODY order when a structural child re-enters the
/// block lowerer. An explicit BODY end may have emitted the suffix already.
fn emit_structural_function<'a>(
    node: &'a Node,
    posts: &ScopePostState,
    emit: &mut impl FnMut(Event<'a>),
) {
    // CVS mdoc_html.c::mdoc_fo_pre/post render HEAD, opening BODY mark,
    // children, and closing BODY mark in that order.
    let head_part = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Head);
    let body_part = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Body && child.scope_end.is_none());
    let synopsis =
        node.flags.synopsis_pretty || body_part.is_some_and(|part| part.flags.synopsis_pretty);
    if let Some(part) = body_part {
        posts.register_function(part.id, synopsis);
    }
    if let Some(part) = head_part {
        emit(Event::BeginNode(part));
        emit(Event::EnterFont(RoffFont::Strong, None));
        emit(Event::Children(&part.children));
        emit(Event::ExitFont(None));
    }
    if let Some(part) = body_part {
        emit(Event::At(part, false));
    }
    emit(Event::Tight);
    emit(Event::Glyph("(".to_owned()));
    emit(Event::Tight);
    if let Some(part) = body_part {
        emit_function_body(&part.children, emit);
    }
    if body_part.is_none_or(|part| !posts.ended(part.id)) {
        if let Some(part) = body_part {
            emit(Event::At(part, false));
        }
        emit(Event::Tight);
        emit(Event::Glyph(if synopsis { ");" } else { ")" }.to_owned()));
    }
}

/// CVS `mdoc_html.c::mdoc_fa_pre()` emits a comma after each operand and when
/// the next logical sibling is another Fa. Preserve the direct Fo body
/// relation even while structural children enter a separate output buffer.
fn emit_function_body<'a>(body: &'a [Node], emit: &mut impl FnMut(Event<'a>)) {
    let mut pending = 0;
    for (index, node) in body.iter().enumerate() {
        if node.macro_name.as_deref() != Some("Fa") {
            continue;
        }
        if pending < index {
            emit(Event::Children(&body[pending..index]));
        }
        let comma_after = !node.children.is_empty()
            && super::adjacency::next(&body[index + 1..])
                .is_some_and(|next| next.macro_name.as_deref() == Some("Fa"));
        emit(Event::FunctionArgument(node, comma_after));
        pending = index + 1;
    }
    if pending < body.len() {
        emit(Event::Children(&body[pending..]));
    }
}

fn resolved_enclosure_marks(node: &Node) -> Option<(Option<String>, Option<String>)> {
    node.enclosure.as_ref().map_or_else(
        || {
            let name = node.macro_name.as_deref()?;
            if name == "En" {
                Some((None, None))
            } else {
                enclosure_marks(name)
                    .map(|(opening, closing)| (Some(opening.to_owned()), Some(closing.to_owned())))
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
    )
}

fn emit_enclosure_post<'a>(close: Option<&str>, emit: &mut impl FnMut(Event<'a>)) {
    if let Some(close) = close {
        emit(Event::Tight);
        if close.is_empty() {
            emit(Event::EmptyWord);
        } else {
            emit(Event::Glyph(close.to_owned()));
        }
    } else {
        // mdoc_term.c releases NOSPACE after an absent `.En` closing mark.
        emit(Event::Release);
    }
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
fn authored_enclosure<'a>(
    node: &'a Node,
    posts: &ScopePostState,
    emit: &mut impl FnMut(Event<'a>),
) {
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
                } else if tail.is_empty() && !posts.ended(child.id) {
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
    is_structural_payload(node) || node.children.iter().any(has_structural_payload)
}

fn is_structural_payload(node: &Node) -> bool {
    node.kind == NodeKind::Table
        || matches!(
            node.macro_name.as_deref(),
            Some("ce" | "rj" | "nf" | "fi" | "EX" | "EE")
        )
        || (node.kind == NodeKind::Block
            && matches!(
                node.macro_name.as_deref(),
                Some("Bl" | "Rs" | "Bd" | "D1" | "Dl")
            ))
}
