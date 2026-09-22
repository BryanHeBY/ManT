//! Commit settled legacy formatter output into the authoritative content store.

use std::{cell::RefCell, rc::Rc};

use mant_ir::{
    ContentAtomKey, ContentOwnerKind, ContentRef, ContentRootKind, ContentStore,
    ContentStoreBuilder, ContentStyle, Inline, LinkOccurrenceKey, LinkTarget, PointBoundary,
    Provenance, SourceSpan,
};

use super::inline::DraftInline;

#[derive(Clone, Debug, Default)]
pub(super) struct LegacyContent {
    builder: Rc<RefCell<ContentStoreBuilder>>,
}

impl LegacyContent {
    pub(super) fn attach_link(
        &self,
        nodes: &[Inline],
        target: LinkTarget,
        title: Option<String>,
    ) -> Option<LinkOccurrenceKey> {
        let mut atoms = Vec::new();
        collect_unlinked_atoms(nodes, &mut atoms)?;
        self.builder
            .borrow_mut()
            .push_link_for_atoms(&atoms, target, title, Provenance::Unknown)
    }

    pub(super) fn text(&self, content: ContentRef) -> Option<String> {
        self.builder.borrow().text(content).map(str::to_owned)
    }

    pub(super) fn replace_text(&self, content: &mut ContentRef, replacement: String) -> bool {
        self.builder.borrow_mut().replace_text(content, replacement)
    }

    pub(super) fn point(
        &self,
        kind: ContentRootKind,
        source: Option<SourceSpan>,
    ) -> mant_ir::ContentPointKey {
        let provenance = provenance(source);
        let builder = &mut *self.builder.borrow_mut();
        let owner = builder.push_owner(owner_kind(kind), provenance);
        let root = builder.push_root(owner, kind, provenance);
        builder.push_point(
            root,
            PointBoundary::BetweenAtoms { atom_boundary: 0 },
            0,
            provenance,
        )
    }

    pub(super) fn anchors(
        &self,
        kind: ContentRootKind,
        targets: impl IntoIterator<Item = String>,
        source: Option<SourceSpan>,
    ) -> Vec<Inline> {
        self.lower(
            kind,
            source,
            targets
                .into_iter()
                .map(|id| DraftInline::anchor_at(id, source))
                .collect(),
        )
    }

    pub(super) fn lower(
        &self,
        kind: ContentRootKind,
        source: Option<SourceSpan>,
        nodes: Vec<DraftInline>,
    ) -> Vec<Inline> {
        let provenance = provenance(source);
        let builder = &mut *self.builder.borrow_mut();
        let owner = builder.push_owner(owner_kind(kind), provenance);
        let root = builder.push_root(owner, kind, provenance);
        let mut state = CommitState {
            builder,
            owner,
            root,
            provenance,
            atom_boundary: 0,
            scalar_boundary: 0,
            style: ContentStyle::default(),
            link: None,
        };
        state.nodes(nodes)
    }

    pub(super) fn with_context<T>(&self, read: impl FnOnce(mant_ir::ContentContext<'_>) -> T) -> T {
        let builder = self.builder.borrow();
        read(builder.content_store().content())
    }

    pub(super) fn finish(&self) -> ContentStore {
        std::mem::take(&mut *self.builder.borrow_mut()).finish()
    }
}

fn collect_unlinked_atoms(nodes: &[Inline], atoms: &mut Vec<ContentAtomKey>) -> Option<()> {
    for node in nodes {
        match node {
            Inline::Text { content } | Inline::Code { content } => atoms.push(content.atom),
            Inline::LineBreak { atom } => atoms.push(*atom),
            Inline::Strong { children } | Inline::Emphasis { children } => {
                collect_unlinked_atoms(children, atoms)?;
            }
            Inline::Anchor { .. } => {}
            Inline::Link { .. } => return None,
        }
    }
    (!atoms.is_empty()).then_some(())
}

struct CommitState<'a> {
    builder: &'a mut ContentStoreBuilder,
    owner: mant_ir::ContentOwnerKey,
    root: mant_ir::ContentRootKey,
    provenance: Provenance,
    atom_boundary: u32,
    scalar_boundary: u32,
    style: ContentStyle,
    link: Option<LinkOccurrenceKey>,
}

impl CommitState<'_> {
    fn nodes(&mut self, nodes: Vec<DraftInline>) -> Vec<Inline> {
        nodes.into_iter().map(|node| self.node(node)).collect()
    }

    fn node(&mut self, node: DraftInline) -> Inline {
        match node {
            DraftInline::Text { value } => {
                let scalar_len = u32::try_from(value.chars().count()).unwrap_or(u32::MAX);
                let content = self.builder.push_text(
                    self.root,
                    value,
                    None,
                    self.style,
                    None,
                    self.link,
                    self.provenance,
                );
                self.advance_atom(scalar_len);
                Inline::Text { content }
            }
            DraftInline::Code { value } => {
                let scalar_len = u32::try_from(value.chars().count()).unwrap_or(u32::MAX);
                let content = self.builder.push_text(
                    self.root,
                    value,
                    None,
                    ContentStyle {
                        literal: true,
                        ..self.style
                    },
                    None,
                    self.link,
                    self.provenance,
                );
                self.advance_atom(scalar_len);
                Inline::Code { content }
            }
            DraftInline::Strong { children } => {
                let previous = self.style.strong;
                self.style.strong = true;
                let children = self.nodes(children);
                self.style.strong = previous;
                Inline::Strong { children }
            }
            DraftInline::Emphasis { children } => {
                let previous = self.style.emphasis;
                self.style.emphasis = true;
                let children = self.nodes(children);
                self.style.emphasis = previous;
                Inline::Emphasis { children }
            }
            DraftInline::Link {
                target,
                title,
                children,
            } => {
                let occurrence = self
                    .builder
                    .push_link(self.owner, target, title, self.provenance);
                let previous = self.link.replace(occurrence);
                let children = self.nodes(children);
                self.link = previous;
                Inline::Link {
                    occurrence,
                    children,
                }
            }
            DraftInline::Anchor { id, owner_source } => {
                let point = self.builder.push_point(
                    self.root,
                    PointBoundary::BetweenAtoms {
                        atom_boundary: self.atom_boundary,
                    },
                    self.scalar_boundary,
                    provenance(owner_source),
                );
                Inline::anchor_at(point, id)
            }
            DraftInline::LineBreak => {
                let atom = self
                    .builder
                    .push_hard_break(self.root, self.link, self.provenance);
                self.advance_atom(1);
                Inline::LineBreak { atom }
            }
        }
    }

    fn advance_atom(&mut self, scalars: u32) {
        self.atom_boundary = self.atom_boundary.saturating_add(1);
        self.scalar_boundary = self.scalar_boundary.saturating_add(scalars);
    }
}

const fn owner_kind(kind: ContentRootKind) -> ContentOwnerKind {
    match kind {
        ContentRootKind::Term => ContentOwnerKind::DefinitionItem,
        ContentRootKind::Cell => ContentOwnerKind::TableCell,
        ContentRootKind::Heading | ContentRootKind::Body | ContentRootKind::FixedBody => {
            ContentOwnerKind::Content
        }
    }
}

const fn provenance(source: Option<SourceSpan>) -> Provenance {
    match source {
        Some(span) => Provenance::Authored { span },
        None => Provenance::Unknown,
    }
}
