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
    /// Return after this node's post; geometry restores after font/post.
    EndNode(&'a Node),
    /// A validated native target attached to the current text destination.
    Anchor(String, Option<mant_ir::SourceSpan>),
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
    /// Restore the font depth saved on the original BODY before its post.
    RestoreBody(u32),
    /// `.Ed` restores the parser's saved fill mode at its source marker.
    RestoreFill(u32, &'a Node),
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
    fonts: FontFrames,
    display_fill: HashMap<u32, bool>,
    indexed: bool,
}

/// One mark/restore record for every font checkpoint, the analog of the
/// upstream single formatter font chain (`termp->fontq` in `term.c`, the
/// `metaf` tag scopes in `html.c`): BODY entries and nested scope opens
/// push frames carrying the saved scope.
///
/// The two frame kinds keep CVS's two distinct close rules on one record:
/// closing a scope truncates every scope frame above its mark, exactly like
/// `print_tagq()` closing opened tags, while an explicitly ended BODY
/// removes exactly its own frame wherever it sits (`mdoc_term.c` retains
/// each BODY's `prev_font` and later closes an inner BODY against its own
/// checkpoint). Because each close only ever matches its own kind, the
/// interleaved record behaves as the two former independent stacks.
#[derive(Default)]
struct FontFrames {
    frames: Vec<FontFrame>,
    /// Open BODY ids; a nested or repeated BODY never re-checkpoints.
    active_bodies: HashSet<u32>,
}

#[derive(Clone, Copy)]
struct FontFrame {
    body: u32,
    kind: FontFrameKind,
    saved: FontScope,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FontFrameKind {
    /// A BODY entry checkpoint; one per open BODY id.
    Body,
    /// A nested font scope open; several may share one BODY.
    Scope,
}

impl FontFrames {
    fn enter_body(&mut self, body: u32, saved: FontScope) {
        if self.active_bodies.insert(body) {
            self.frames.push(FontFrame {
                body,
                kind: FontFrameKind::Body,
                saved,
            });
        }
    }

    fn exit_body(&mut self, body: u32) -> Option<FontScope> {
        let index = self
            .frames
            .iter()
            .rposition(|frame| frame.body == body && frame.kind == FontFrameKind::Body)?;
        let saved = self.frames.remove(index).saved;
        self.active_bodies.remove(&body);
        Some(saved)
    }

    fn enter_scope(&mut self, body: u32, saved: FontScope) {
        self.frames.push(FontFrame {
            body,
            kind: FontFrameKind::Scope,
            saved,
        });
    }

    fn exit_scope(&mut self, body: u32) -> Option<FontScope> {
        let mark = self
            .frames
            .iter()
            .rposition(|frame| frame.body == body && frame.kind == FontFrameKind::Scope)?;
        let saved = self.frames[mark].saved;
        // mdoc_term.c::print_mdoc_node() pops to the original BODY's
        // prev_font, including any crossed scopes above that BODY. BODY
        // frames stay open wherever the walk opened them; each kind's close
        // only ever matches its own kind, so the shared record behaves as
        // the two former independent stacks.
        for frame in self.frames.split_off(mark) {
            if frame.kind == FontFrameKind::Body {
                self.frames.push(frame);
            }
        }
        Some(saved)
    }
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
        self.0.borrow_mut().fonts.enter_scope(body_id, saved);
    }

    pub(super) fn exit_font(&self, body_id: u32) -> Option<FontScope> {
        self.0.borrow_mut().fonts.exit_scope(body_id)
    }

    pub(super) fn enter_body(&self, body_id: u32, saved: FontScope) {
        self.0.borrow_mut().fonts.enter_body(body_id, saved);
    }

    pub(super) fn exit_body(&self, body_id: u32) -> Option<FontScope> {
        self.0.borrow_mut().fonts.exit_body(body_id)
    }

    pub(super) fn enter_display_fill(&self, body_id: u32, inbound: bool) {
        self.0.borrow_mut().display_fill.insert(body_id, inbound);
    }

    pub(super) fn exit_display_fill(&self, body_id: u32) -> Option<bool> {
        self.0.borrow_mut().display_fill.remove(&body_id)
    }
}

/// The native renderer has one node dispatch and one font stack regardless
/// of whether the current IR destination is a paragraph, a list head, or a
/// literal display.  Sinks choose only where executed output is written.
pub(super) trait ContainerSink<'a> {
    fn font(&mut self) -> &mut super::inline::FontState;
    fn source_node(&mut self, node: &'a Node, starts_line: bool);
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
            if let Some(index) = geometry.iter().rposition(|(id, _)| *id == source.id) {
                let (_, checkpoint) = geometry.remove(index);
                sink.restore_geometry(checkpoint);
            }
        }
        Event::RestoreBody(body_id) => {
            if let Some(saved) = posts.exit_body(body_id) {
                sink.font().pop_scope(saved);
            }
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
            node.macro_name.as_deref(),
            Some("Bf" | "Bk" | "Fo" | "Xo" | "ce" | "rj")
        )
        || super::inline::is_enclosure_macro(node.macro_name.as_deref())
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
    emit(Event::At(node, true));
    if walk_scope_end(node, posts, &mut emit) {
        emit(Event::EndNode(node));
        return true;
    }
    match node.macro_name.as_deref() {
        Some("Bf" | "Bk") => emit_font_or_keep(node, &mut emit),
        Some("Fo") => {
            emit_structural_function(node, posts, &mut emit);
        }
        Some("ce" | "rj") => aligned_line_payload(node, &mut emit),
        Some("Eo") => authored_enclosure(node, posts, &mut emit),
        Some("Xo") => transparent_scope(node, posts, &mut emit),
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
    let body = node
        .children
        .iter()
        .find(|part| part.kind == NodeKind::Body);
    let body_id = body.map(|part| part.id);
    if node.macro_name.as_deref() == Some("Bk") {
        emit(Event::EnterKeep);
    } else if let Some(font) = node.font {
        emit(Event::EnterFont(font.into(), body_id));
    }
    if let Some(body) = body {
        emit(Event::At(body, false));
        emit(Event::Children(&body.children));
        emit(Event::RestoreBody(body.id));
        emit(Event::EndNode(body));
    }
    if node.macro_name.as_deref() == Some("Bk") {
        emit(Event::ExitKeep);
    } else if node.font.is_some() {
        emit(Event::ExitFont(body_id));
    }
}

fn emit_enclosure<'a>(
    node: &'a Node,
    posts: &ScopePostState,
    emit: &mut impl FnMut(Event<'a>),
) -> bool {
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
    let body = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Body);
    if let Some(body) = body {
        emit(Event::At(body, false));
        emit(Event::Children(&body.children));
        let original_body_id = body.scope_end.is_none().then_some(body.id);
        if original_body_id.is_none_or(|id| !posts.ended(id)) {
            if let Some(body_id) = original_body_id {
                emit(Event::RestoreBody(body_id));
            }
            emit_enclosure_post(close.as_deref(), emit);
        }
        emit(Event::EndNode(body));
        for child in node
            .children
            .iter()
            .filter(|child| child.flags.delimiter_close)
        {
            emit(Event::Children(std::slice::from_ref(child)));
        }
    } else {
        emit(Event::Children(&node.children));
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
    if node.macro_name.as_deref() == Some("Bf") {
        emit(Event::Children(&node.children));
        emit(Event::RestoreBody(end.body_id));
        emit(Event::ExitFont(Some(end.body_id)));
        posts.finish(end.body_id);
        return true;
    }
    if node.macro_name.as_deref() == Some("Fo") {
        emit(Event::Children(&node.children));
        emit(Event::RestoreBody(end.body_id));
        if let Some(synopsis) = posts.function_suffix(end.body_id) {
            emit(Event::Tight);
            emit(Event::Glyph(if synopsis { ");" } else { ")" }.to_owned()));
            posts.finish(end.body_id);
        }
        return true;
    }
    if node.macro_name.as_deref() == Some("Eo") {
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
    if node.macro_name.as_deref() == Some("Bd") {
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
        if let Some(target) = super::targets::raw_target(part) {
            emit(Event::Anchor(target, super::source_span(part)));
        }
        emit(Event::EnterFont(RoffFont::Strong, None));
        emit(Event::Children(&part.children));
        emit(Event::ExitFont(None));
        emit(Event::EndNode(part));
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
            emit(Event::RestoreBody(part.id));
        }
        emit(Event::Tight);
        emit(Event::Glyph(if synopsis { ");" } else { ")" }.to_owned()));
    }
    if let Some(part) = body_part {
        emit(Event::EndNode(part));
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
                enclosure_marks(name).map(|(opening, closing)| {
                    // mdoc_term.c::termp_quote_pre/post (1600-1603, 1658-1661)
                    // print ASCII angle brackets when the enclosure's sole
                    // child is an `.Mt`; every other child shape takes the
                    // device glyph pair from the character catalog.
                    if matches!(name, "Aq" | "Ao") && sole_mt_child(node) {
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
        [only] => only.macro_name.as_deref() == Some("Mt"),
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
            node.macro_name.as_deref(),
            Some("ce" | "rj" | "nf" | "fi" | "EX" | "EE")
        )
        || (node.kind == NodeKind::Block
            && matches!(
                node.macro_name.as_deref(),
                Some("Bl" | "Rs" | "Bd" | "D1" | "Dl")
            ))
}
