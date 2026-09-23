//! Shared key-backed content fixtures for UI unit tests.

use std::{cell::RefCell, collections::BTreeSet};

use mant_ir::{
    ContentAtomKey, ContentOwnerKey, ContentOwnerKind, ContentRootKey, ContentRootKind,
    ContentStore, ContentStoreBuilder, ContentStyle, FragmentAlias, Inline, LinkTarget, NodeId,
    Provenance,
};

struct Fixture {
    builder: ContentStoreBuilder,
    owner: ContentOwnerKey,
    root: ContentRootKey,
}

impl Fixture {
    fn new() -> Self {
        let mut builder = ContentStoreBuilder::new();
        let owner = builder.push_owner(ContentOwnerKind::Document, Provenance::Unknown);
        let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        Self {
            builder,
            owner,
            root,
        }
    }
}

thread_local! {
    static FIXTURE: RefCell<Fixture> = RefCell::new(Fixture::new());
}

pub(crate) fn text(value: impl Into<String>) -> Inline {
    FIXTURE.with(|fixture| {
        let mut fixture = fixture.borrow_mut();
        let root = fixture.root;
        let content = fixture.builder.push_text(
            root,
            value.into(),
            None,
            ContentStyle::default(),
            None,
            None,
            Provenance::Unknown,
        );
        Inline::Text { content }
    })
}

pub(crate) fn code(value: impl Into<String>) -> Inline {
    FIXTURE.with(|fixture| {
        let mut fixture = fixture.borrow_mut();
        let root = fixture.root;
        let content = fixture.builder.push_text(
            root,
            value.into(),
            None,
            ContentStyle {
                literal: true,
                ..ContentStyle::default()
            },
            None,
            None,
            Provenance::Unknown,
        );
        Inline::Code { content }
    })
}

pub(crate) fn line_break() -> Inline {
    FIXTURE.with(|fixture| {
        let mut fixture = fixture.borrow_mut();
        let root = fixture.root;
        Inline::LineBreak {
            atom: fixture
                .builder
                .push_hard_break(root, None, Provenance::Unknown),
        }
    })
}

pub(crate) fn anchor(id: impl Into<NodeId>) -> Inline {
    anchor_with_aliases(id, Vec::new())
}

pub(crate) fn anchor_with_aliases(id: impl Into<NodeId>, aliases: Vec<FragmentAlias>) -> Inline {
    FIXTURE.with(|fixture| {
        let mut fixture = fixture.borrow_mut();
        let root = fixture.root;
        let atom_boundary = fixture
            .builder
            .content_store()
            .root(root)
            .map_or(0, |root| {
                u32::try_from(root.atoms.len()).unwrap_or(u32::MAX)
            });
        let scalar_boundary = fixture
            .builder
            .content_store()
            .root_logical_text(root)
            .map_or(0, |text| {
                u32::try_from(text.chars().count()).unwrap_or(u32::MAX)
            });
        let point = fixture.builder.push_point(
            root,
            mant_ir::PointBoundary::BetweenAtoms { atom_boundary },
            scalar_boundary,
            Provenance::Generated { trigger: None },
        );
        Inline::anchor_with_aliases(point, id, aliases)
    })
}

pub(crate) fn link(target: LinkTarget, title: Option<String>, children: Vec<Inline>) -> Inline {
    let atoms = inline_atoms(&children);
    FIXTURE.with(|fixture| {
        let mut fixture = fixture.borrow_mut();
        let occurrence = if atoms.is_empty() {
            let owner = fixture.owner;
            fixture
                .builder
                .push_link(owner, target, title, Provenance::Unknown)
        } else {
            fixture
                .builder
                .push_link_for_atoms(&atoms, target, title, Provenance::Unknown)
                .expect("fixture link atoms are unlinked and ordered")
        };
        Inline::Link {
            occurrence,
            children,
        }
    })
}

fn inline_atoms(nodes: &[Inline]) -> Vec<ContentAtomKey> {
    fn collect(nodes: &[Inline], atoms: &mut BTreeSet<ContentAtomKey>) {
        for node in nodes {
            match node {
                Inline::Text { content } | Inline::Code { content } => {
                    atoms.insert(content.atom);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => collect(children, atoms),
                Inline::LineBreak { atom } => {
                    atoms.insert(*atom);
                }
                Inline::Anchor { .. } => {}
            }
        }
    }
    let mut atoms = BTreeSet::new();
    collect(nodes, &mut atoms);
    atoms.into_iter().collect()
}

pub(crate) fn store() -> ContentStore {
    FIXTURE.with(|fixture| fixture.borrow().builder.content_store().clone())
}

pub(crate) fn sync_document(document: &mut mant_ir::Document) {
    document
        .flow_mut()
        .expect("test document has a Flow body")
        .content_store = store();
}

pub(crate) fn content() -> mant_ir::ContentContext<'static> {
    Box::leak(Box::new(store())).content()
}

pub(crate) fn heading(value: impl Into<String>) -> mant_ir::Heading {
    mant_ir::Heading {
        content: vec![text(value)],
        source: None,
    }
}
