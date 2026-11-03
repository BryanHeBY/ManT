//! One shared record for BODY posts, font checkpoints and display fill.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use libmandoc_rs::Node;

use super::{has_structural_payload, is_structural_payload};
use crate::mandoc::inline::{FontScope, FontState};

/// Private execution record for mdoc BODY posts. Clones share one source-order
/// walk even when a list or display temporarily owns a separate output buffer.
#[derive(Clone, Default)]
pub(in crate::mandoc) struct ScopePostState(Rc<RefCell<ScopePosts>>);

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
    pub(in crate::mandoc) fn index_structural_payload(&self, root: &Node) {
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

    pub(in crate::mandoc) fn has_structural_payload(&self, node: &Node) -> bool {
        let posts = self.0.borrow();
        if posts.indexed {
            posts.structural_payload.contains(&node.id)
        } else {
            // Standalone inline builders in focused tests do not have a
            // document context; their small borrowed subtree is scanned.
            has_structural_payload(node)
        }
    }

    pub(in crate::mandoc) fn ended(&self, body_id: u32) -> bool {
        self.0.borrow().ended.contains(&body_id)
    }

    pub(in crate::mandoc) fn finish(&self, body_id: u32) {
        self.0.borrow_mut().ended.insert(body_id);
    }

    pub(in crate::mandoc) fn register_function(&self, body_id: u32, synopsis: bool) {
        self.0
            .borrow_mut()
            .function_suffix
            .insert(body_id, synopsis);
    }

    pub(in crate::mandoc) fn function_suffix(&self, body_id: u32) -> Option<bool> {
        self.0.borrow().function_suffix.get(&body_id).copied()
    }

    pub(in crate::mandoc) fn enter_font(&self, body_id: u32, saved: FontScope) {
        self.0.borrow_mut().fonts.enter_scope(body_id, saved);
    }

    pub(in crate::mandoc) fn exit_font(&self, body_id: u32) -> Option<FontScope> {
        self.0.borrow_mut().fonts.exit_scope(body_id)
    }

    pub(in crate::mandoc) fn enter_body(&self, body_id: u32, saved: FontScope) {
        self.0.borrow_mut().fonts.enter_body(body_id, saved);
    }

    pub(in crate::mandoc) fn exit_body(&self, body_id: u32) -> Option<FontScope> {
        self.0.borrow_mut().fonts.exit_body(body_id)
    }

    /// Register one display BODY at the caller's existing pre stage. Bd
    /// retains its inbound fill checkpoint; D1/Dl do not own that checkpoint.
    /// Source entry, tab pre and output routing stay with the caller.
    pub(in crate::mandoc) fn enter_display_body(
        &self,
        body_id: u32,
        font: &FontState,
        inbound_fill: Option<bool>,
    ) {
        self.enter_body(body_id, font.checkpoint());
        if let Some(fill) = inbound_fill {
            self.enter_display_fill(body_id, fill);
        }
    }

    /// `print_mdoc_node()` restores the original BODY font before its native
    /// post. Explicit end markers may already have consumed this checkpoint.
    /// The caller still executes post, fill restoration and geometry stages.
    pub(in crate::mandoc) fn restore_body_font(&self, body_id: u32, font: &mut FontState) {
        if let Some(saved) = self.exit_body(body_id) {
            font.pop_scope(saved);
        }
    }

    pub(in crate::mandoc) fn enter_display_fill(&self, body_id: u32, inbound: bool) {
        self.0.borrow_mut().display_fill.insert(body_id, inbound);
    }

    pub(in crate::mandoc) fn exit_display_fill(&self, body_id: u32) -> Option<bool> {
        self.0.borrow_mut().display_fill.remove(&body_id)
    }
}
