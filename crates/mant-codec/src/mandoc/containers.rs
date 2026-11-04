//! Streaming container boundaries shared by inline and structural consumers.
//!
//! Container transparency means that payloads remain reachable. It does not
//! mean that the entire container may be skipped by logical sibling lookup.
use libmandoc_rs::{
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, Node, NodeKind, RoffMacro,
};
mod posts;
pub(super) use posts::ScopePostState;

use super::{first_part_children, inline::enclosure_marks, roff_escape::RoffFont};

pub(super) enum Event<'a> {
    /// Source owner of subsequent generated output. True executes this
    /// node's `NODE_LINE`; false only changes ownership for a later post.
    At(&'a Node, bool),
    /// An executed wrapper boundary whose children are emitted separately.
    BeginNode(&'a Node),
    /// Return after this node's post; geometry restores after font/post.
    EndNode(&'a Node),
    /// A validated native target attached to the current text destination.
    Anchor(String, Option<mant_ir::SourceSpan>),
    /// A formatter flush boundary, distinct from an extra blank row.
    Break,
    /// Emit the current output row even if preceding requests emptied it.
    FlushLine,
    /// Viewport presentation changes without creating another text executor.
    Alignment(mant_ir::TextAlignment),
    Children(&'a [Node]),
    /// `pre_ce` visits `term_word/roff_term_pre` directly, without `NODE_LINE`.
    AlignedChildren(&'a [Node]),
    Glyph(String),
    Tight,
    Release,
    EmptyWord,
    EnterKeep,
    ExitKeep,
    EnterFont(RoffFont, Option<u32>),
    ExitFont(Option<u32>),
    /// Restore the font depth saved on the original BODY before its post.
    RestoreBody(u32),
    /// `.Ed` restores the parser's saved fill mode at its source marker.
    RestoreFill(u32, &'a Node),
    /// A direct Fa child of a structural Fo body; the sibling relation is
    /// needed for mandoc's generated comma after an argument.
    FunctionArgument(&'a Node, bool),
    /// Real Fo BODY pre/post, shared with inline Fn field execution.
    FunctionBodyBegin(bool),
    FunctionBodyEnd(bool),
}

/// The native renderer has one node dispatch and one font stack regardless
/// of whether the current IR destination is a paragraph, a list head, or a
/// literal display.  Sinks choose only where executed output is written.
pub(super) trait ContainerSink<'a> {
    fn font(&mut self) -> &mut super::inline::FontState;
    fn source_node(&mut self, node: &'a Node, starts_line: bool);
    fn finish_node(&mut self, node: &Node);
    fn event(&mut self, event: Event<'a>);
    fn restore_fill(&mut self, _fill: bool, _marker: &Node) {}
    fn geometry_checkpoint(
        &self,
        _node: &Node,
    ) -> Option<super::inline::DefinitionGeometryCheckpoint> {
        None
    }
    fn restore_geometry(
        &mut self,
        _checkpoint: Option<super::inline::DefinitionGeometryCheckpoint>,
    ) {
    }
}

pub(super) fn drive<'a>(
    node: &'a Node,
    posts: &ScopePostState,
    sink: &mut impl ContainerSink<'a>,
) -> bool {
    let mut local_font = None;
    let mut geometry = Vec::new();
    walk(node, posts, |event| match event {
        Event::At(source, starts_line) => {
            if source.kind == NodeKind::Body {
                posts.enter_body(source.id, sink.font().checkpoint());
            }
            sink.source_node(source, starts_line);
            if !geometry.iter().any(|(id, _)| *id == source.id) {
                geometry.push((source.id, sink.geometry_checkpoint(source)));
            }
        }
        Event::BeginNode(source) => {
            if !geometry.iter().any(|(id, _)| *id == source.id) {
                sink.event(Event::BeginNode(source));
                geometry.push((source.id, sink.geometry_checkpoint(source)));
            }
        }
        Event::EndNode(source) => {
            sink.finish_node(source);
            if let Some(index) = geometry.iter().rposition(|(id, _)| *id == source.id) {
                let (_, checkpoint) = geometry.remove(index);
                sink.restore_geometry(checkpoint);
            }
        }
        Event::RestoreBody(body_id) => {
            posts.restore_body_font(body_id, sink.font());
        }
        Event::RestoreFill(body_id, marker) => {
            if let Some(fill) = posts.exit_display_fill(body_id) {
                sink.restore_fill(fill, marker);
            }
        }
        Event::EnterFont(font, body_id) => {
            let saved = sink.font().push_scope(font);
            if let Some(body_id) = body_id {
                posts.enter_font(body_id, saved);
            } else {
                local_font = Some(saved);
            }
        }
        Event::ExitFont(body_id) => {
            let saved = body_id
                .and_then(|id| posts.exit_font(id))
                .or_else(|| body_id.is_none().then(|| local_font.take()).flatten());
            if let Some(saved) = saved {
                sink.font().pop_scope(saved);
            }
        }
        Event::FunctionArgument(argument, comma_after) => {
            sink.source_node(argument, true);
            sink.event(Event::FunctionArgument(argument, comma_after));
        }
        event => sink.event(event),
    })
}

pub(super) fn is_container(node: &Node) -> bool {
    node.scope_end.is_some()
        || matches!(
            node.macro_token.as_ref(),
            Some(
                Mdoc(MdocMacro::Bf | MdocMacro::Bk | MdocMacro::Fo | MdocMacro::Xo)
                    | Roff(RoffMacro::Ce | RoffMacro::Rj)
            )
        )
        || super::inline::is_enclosure_macro(node.macro_token.as_ref())
        || (node.macro_token.is_none()
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
    emit(Event::At(node, true));
    if walk_scope_end(node, posts, &mut emit) {
        emit(Event::EndNode(node));
        return true;
    }
    match node.macro_token.as_ref() {
        Some(Mdoc(MdocMacro::Bf | MdocMacro::Bk)) => emit_font_or_keep(node, &mut emit),
        Some(Mdoc(MdocMacro::Fo)) => {
            emit_structural_function(node, posts, &mut emit);
        }
        Some(Roff(RoffMacro::Ce | RoffMacro::Rj)) => aligned_line_payload(node, &mut emit),
        Some(Mdoc(MdocMacro::Eo)) => authored_enclosure(node, posts, &mut emit),
        Some(Mdoc(MdocMacro::Xo)) => transparent_scope(node, posts, &mut emit),
        name if super::inline::is_enclosure_macro(name) => {
            if !emit_enclosure(node, posts, &mut emit) {
                return false;
            }
        }
        None if matches!(
            node.kind,
            NodeKind::Root | NodeKind::Head | NodeKind::Body | NodeKind::Tail
        ) =>
        {
            emit(Event::Children(&node.children));
            if node.kind == NodeKind::Body {
                emit(Event::RestoreBody(node.id));
            }
        }
        _ => return false,
    }
    emit(Event::EndNode(node));
    true
}

/// `Xo`/`Xc` have no macro pre/post in `mdoc_term.c`. Visit their actual
/// structural parts, preserving the shared node-entry/font lifecycle, but
/// never turn an output-owner return into `term_newln()`/`term_flushln()`.
fn transparent_scope<'a>(node: &'a Node, posts: &ScopePostState, emit: &mut impl FnMut(Event<'a>)) {
    for child in &node.children {
        if matches!(child.kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail) {
            emit(Event::At(child, true));
            emit(Event::Children(&child.children));
            if child.kind == NodeKind::Body && !posts.ended(child.id) {
                emit(Event::RestoreBody(child.id));
            }
            emit(Event::EndNode(child));
        } else {
            emit(Event::Children(std::slice::from_ref(child)));
        }
    }
}

fn emit_font_or_keep<'a>(node: &'a Node, emit: &mut impl FnMut(Event<'a>)) {
    // termp_bf_pre/termp_bk_pre skip HEAD and select font/keep only at
    // BODY. blk_full() may leave direct punctuation before HEAD; it runs
    // in the enclosing font before that BODY pre, in the same buffer.
    for child in &node.children {
        match child.kind {
            NodeKind::Head => {
                emit(Event::At(child, true));
                emit(Event::EndNode(child));
            }
            NodeKind::Body => {
                emit(Event::At(child, true));
                if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bk)) {
                    emit(Event::EnterKeep);
                } else if let Some(font) = node.font {
                    emit(Event::EnterFont(font.into(), Some(child.id)));
                }
                emit(Event::Children(&child.children));
                emit(Event::RestoreBody(child.id));
                if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bk)) {
                    emit(Event::ExitKeep);
                } else if node.font.is_some() {
                    emit(Event::ExitFont(Some(child.id)));
                }
                emit(Event::EndNode(child));
            }
            _ => emit(Event::Children(std::slice::from_ref(child))),
        }
    }
}

fn emit_enclosure<'a>(
    node: &'a Node,
    posts: &ScopePostState,
    emit: &mut impl FnMut(Event<'a>),
) -> bool {
    // CVS termp_quote_pre/post act on BODY/ELEM only. The BLOCK's direct
    // children are a complete source-ordered stream: punctuation may sit
    // before its HEAD or after its BODY without DELIMO/DELIMC flags.
    if !matches!(node.kind, NodeKind::Body | NodeKind::Element) {
        emit(Event::Children(&node.children));
        return true;
    }
    let Some((open, close)) = resolved_enclosure_marks(node) else {
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
    emit(Event::Children(&node.children));
    if !posts.ended(node.id) {
        if node.kind == NodeKind::Body {
            emit(Event::RestoreBody(node.id));
        }
        emit_enclosure_post(close.as_deref(), emit);
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
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bf)) {
        emit(Event::Children(&node.children));
        emit(Event::RestoreBody(end.body_id));
        emit(Event::ExitFont(Some(end.body_id)));
        posts.finish(end.body_id);
        return true;
    }
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fo)) {
        emit(Event::Children(&node.children));
        emit(Event::RestoreBody(end.body_id));
        if let Some(synopsis) = posts.function_suffix(end.body_id) {
            emit(Event::FunctionBodyEnd(synopsis));
            posts.finish(end.body_id);
        }
        return true;
    }
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Eo)) {
        if !node.children.is_empty() {
            emit(Event::BeginNode(node));
            emit(Event::Tight);
        }
        emit(Event::Children(&node.children));
        emit(Event::RestoreBody(end.body_id));
        emit(Event::Release);
        posts.finish(end.body_id);
        return true;
    }
    if let Some((_, close)) = resolved_enclosure_marks(node) {
        if !node.children.is_empty() {
            emit(Event::BeginNode(node));
        }
        emit(Event::Children(&node.children));
        emit(Event::RestoreBody(end.body_id));
        emit_enclosure_post(close.as_deref(), emit);
        posts.finish(end.body_id);
        return true;
    }
    emit(Event::Children(&node.children));
    emit(Event::RestoreBody(end.body_id));
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bd)) {
        emit(Event::RestoreFill(end.body_id, node));
    }
    posts.finish(end.body_id);
    true
}

/// Keep the native Fo HEAD/BODY order when a structural child re-enters the
/// block lowerer. An explicit BODY end may have emitted the suffix already.
fn emit_structural_function<'a>(
    node: &'a Node,
    posts: &ScopePostState,
    emit: &mut impl FnMut(Event<'a>),
) {
    // termp_fo_pre/post attach bold to HEAD and punctuation to BODY.
    // Direct BLOCK children execute around those parts in source order.
    for part in &node.children {
        match part.kind {
            NodeKind::Head => {
                emit(Event::BeginNode(part));
                if let Some(target) = super::targets::raw_target(part) {
                    emit(Event::Anchor(target, super::source_span(part)));
                }
                emit(Event::EnterFont(RoffFont::Strong, None));
                emit(Event::Children(&part.children));
                emit(Event::ExitFont(None));
                emit(Event::EndNode(part));
            }
            NodeKind::Body if part.scope_end.is_none() => {
                let synopsis = node.flags.synopsis_pretty || part.flags.synopsis_pretty;
                posts.register_function(part.id, synopsis);
                emit(Event::At(part, true));
                emit(Event::FunctionBodyBegin(synopsis));
                emit_function_body(&part.children, emit);
                if !posts.ended(part.id) {
                    emit(Event::RestoreBody(part.id));
                    emit(Event::FunctionBodyEnd(synopsis));
                }
                emit(Event::EndNode(part));
            }
            _ => emit(Event::Children(std::slice::from_ref(part))),
        }
    }
}

/// CVS `mdoc_html.c::mdoc_fa_pre()` emits a comma after each operand and when
/// the next logical sibling is another Fa. Preserve the direct Fo body
/// relation even while structural children enter a separate output buffer.
fn emit_function_body<'a>(body: &'a [Node], emit: &mut impl FnMut(Event<'a>)) {
    let mut pending = 0;
    for (index, node) in body.iter().enumerate() {
        if node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::Fa)) {
            continue;
        }
        if pending < index {
            emit(Event::Children(&body[pending..index]));
        }
        let comma_after = !node.children.is_empty()
            && super::adjacency::next(&body[index + 1..])
                .is_some_and(|next| next.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fa)));
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
            let name = node.macro_token.as_ref()?;
            if name == &Mdoc(MdocMacro::En) {
                Some((None, None))
            } else {
                enclosure_marks(name).map(|(opening, closing)| {
                    // mdoc_term.c::termp_quote_pre/post (1600-1603, 1658-1661)
                    // print ASCII angle brackets when the enclosure's sole
                    // child is an `.Mt`; every other child shape takes the
                    // device glyph pair from the character catalog.
                    if matches!(name, Mdoc(MdocMacro::Aq | MdocMacro::Ao)) && sole_mt_child(node) {
                        return (Some("<".to_owned()), Some(">".to_owned()));
                    }
                    (Some(opening), Some(closing))
                })
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

fn sole_mt_child(node: &Node) -> bool {
    // termp_quote_pre accepts exactly one `.Mt` child; any sibling — a
    // second `.Mt`, a `.No`, … — falls back to the catalog glyph pair.
    match super::inline::inline_children(node) {
        [only] => only.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Mt)),
        _ => false,
    }
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
    emit(Event::Alignment(
        if node.macro_token == Some(Roff(RoffMacro::Rj)) {
            mant_ir::TextAlignment::Right
        } else {
            mant_ir::TextAlignment::Center
        },
    ));
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
                children[index].macro_token.as_ref(),
                Some(Roff(
                    RoffMacro::Br | RoffMacro::Fi | RoffMacro::Nf | RoffMacro::Ti
                ))
            ) {
                if pending < index {
                    emit(Event::AlignedChildren(&children[pending..index]));
                }
                emit(Event::Break);
                pending = index;
            }
        }
        emit(Event::AlignedChildren(&children[pending..end]));
        emit(Event::FlushLine);
        start = end;
    }
    emit(Event::Alignment(mant_ir::TextAlignment::Left));
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
            emit(Event::At(child, true));
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
                if !posts.ended(child.id) {
                    emit(Event::RestoreBody(child.id));
                }
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
        if matches!(child.kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail) {
            emit(Event::EndNode(child));
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
            node.macro_token.as_ref(),
            Some(
                Roff(RoffMacro::Ce | RoffMacro::Rj | RoffMacro::Nf | RoffMacro::Fi)
                    | Man(ManMacro::Ex | ManMacro::Ee)
            )
        )
        || (node.kind == NodeKind::Block
            && matches!(
                node.macro_token.as_ref(),
                Some(Mdoc(
                    MdocMacro::Bl | MdocMacro::Rs | MdocMacro::Bd | MdocMacro::D1 | MdocMacro::Dl
                ))
            ))
}
