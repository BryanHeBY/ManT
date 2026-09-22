use std::collections::HashSet;

use crate::{LinkTarget, Provenance};

use super::{
    ContentAtom, ContentAtomKey, ContentAtomKind, ContentByteRange, ContentOwner, ContentOwnerKey,
    ContentOwnerKind, ContentPoint, ContentPointKey, ContentRef, ContentRole, ContentRoot,
    ContentRootKey, ContentRootKind, ContentStore, ContentStyle, LinkLabelPart, LinkOccurrence,
    LinkOccurrenceKey, PointBoundary,
};

pub(super) fn detach_link(store: &mut ContentStore, key: LinkOccurrenceKey) -> bool {
    let Some(link) = store.link(key) else {
        return false;
    };
    let atoms = link
        .label
        .iter()
        .map(|part| match part {
            LinkLabelPart::Content { content } => content.atom,
            LinkLabelPart::HardBreak { atom } => *atom,
        })
        .collect::<Vec<_>>();
    let mut unique = HashSet::with_capacity(atoms.len());
    if atoms.iter().any(|atom| {
        !unique.insert(*atom)
            || store
                .atom(*atom)
                .is_none_or(|record| record.link != Some(key))
    }) {
        return false;
    }
    store
        .links
        .get_mut(key.index().expect("link key fits usize"))
        .expect("prevalidated link remains allocated")
        .label
        .clear();
    for atom in atoms {
        store
            .atoms
            .get_mut(atom.index().expect("atom key fits usize"))
            .expect("prevalidated linked atom remains allocated")
            .link = None;
    }
    true
}

/// Incremental constructor used by trusted document producers.
///
/// The builder assigns dense keys and maintains owner/root membership at the
/// same time as records are added.  [`crate::validate_content_store`] remains
/// the admission boundary for decoded or otherwise untrusted stores.
#[derive(Debug, Default)]
pub struct ContentStoreBuilder {
    store: ContentStore,
}

impl ContentStoreBuilder {
    /// Create an empty store builder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            store: ContentStore {
                owners: Vec::new(),
                roots: Vec::new(),
                atoms: Vec::new(),
                points: Vec::new(),
                links: Vec::new(),
            },
        }
    }

    /// Borrow the partially built store for producer-side contextual reads.
    #[must_use]
    pub const fn content_store(&self) -> &ContentStore {
        &self.store
    }

    /// Resolve text already allocated by this builder.
    #[must_use]
    pub fn text(&self, content: ContentRef) -> Option<&str> {
        self.store.text(content)
    }

    /// Resolve a link occurrence already allocated by this builder.
    #[must_use]
    pub fn link(&self, key: LinkOccurrenceKey) -> Option<&LinkOccurrence> {
        self.store.link(key)
    }

    /// Mutably resolve a link occurrence already allocated by this builder.
    #[must_use]
    pub fn link_mut(&mut self, key: LinkOccurrenceKey) -> Option<&mut LinkOccurrence> {
        self.store
            .links
            .get_mut(key.index()?)
            .filter(|record| record.key == key)
    }

    /// Attach one new occurrence to already allocated, currently unlinked atoms.
    ///
    /// This supports producer-side semantic normalization after formatter
    /// output has settled, without copying the label into a second content
    /// root. All atoms must belong to one owner and appear in retained order.
    #[must_use]
    pub fn push_link_for_atoms(
        &mut self,
        atoms: &[ContentAtomKey],
        target: LinkTarget,
        title: Option<String>,
        provenance: Provenance,
    ) -> Option<LinkOccurrenceKey> {
        let first = *atoms.first()?;
        let owner = self.store.atom(first)?.owner;
        let mut previous = None;
        let mut label = Vec::with_capacity(atoms.len());
        for &key in atoms {
            let atom = self.store.atom(key)?;
            if atom.owner != owner
                || atom.link.is_some()
                || previous.is_some_and(|prev| prev >= key)
            {
                return None;
            }
            match &atom.kind {
                ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
                    label.push(LinkLabelPart::Content {
                        content: ContentRef {
                            atom: key,
                            bytes: ContentByteRange {
                                start: 0,
                                end: u32::try_from(text.len()).ok()?,
                            },
                        },
                    });
                }
                ContentAtomKind::HardBreak {} => {
                    label.push(LinkLabelPart::HardBreak { atom: key });
                }
                ContentAtomKind::BreakOpportunity {} => return None,
            }
            if let Some(previous_key) = previous {
                let previous_atom = self.store.atom(previous_key)?;
                if previous_atom.root == atom.root {
                    let root = self.store.root(atom.root)?;
                    // Root atoms are retained in increasing dense-key order.
                    // A long styled link must not rescan the whole root for
                    // every label part.
                    let previous_index = root.atoms.binary_search(&previous_key).ok()?;
                    let current_index = root.atoms.binary_search(&key).ok()?;
                    if previous_index >= current_index
                        || root.atoms[previous_index + 1..current_index]
                            .iter()
                            .any(|gap| {
                                self.store.atom(*gap).is_none_or(|record| {
                                    !matches!(record.kind, ContentAtomKind::BreakOpportunity {})
                                })
                            })
                    {
                        return None;
                    }
                }
            }
            previous = Some(key);
        }
        let key = dense_key(self.store.links.len(), LinkOccurrenceKey::new);
        self.store.links.push(LinkOccurrence {
            key,
            owner,
            target,
            title,
            label,
            provenance,
        });
        for atom in atoms {
            self.store
                .atoms
                .get_mut(atom.index()?)
                .filter(|record| record.key == *atom)
                .map(|record| record.link = Some(key))?;
        }
        Some(key)
    }

    /// Detach a producer-side occurrence while retaining its dense record.
    ///
    /// The now-empty occurrence remains a harmless construction witness; all
    /// of its former atoms become ordinary unlinked content atomically.
    pub fn detach_link(&mut self, key: LinkOccurrenceKey) -> bool {
        detach_link(&mut self.store, key)
    }

    /// Replace a full-atom text reference before any derived slices are made.
    ///
    /// The passed reference is updated to cover the replacement. Partial atom
    /// references are rejected so a producer cannot silently invalidate other
    /// ranges into the same atom. Later zero-width points are shifted by the
    /// logical scalar delta; an in-atom point on the replaced atom rejects the
    /// operation before mutation.
    pub fn replace_text(&mut self, content: &mut ContentRef, replacement: String) -> bool {
        let Some(atom) = self.store.atom(content.atom) else {
            return false;
        };
        let Some(text) = atom.kind.text() else {
            return false;
        };
        if content.bytes.start != 0 || content.bytes.end as usize != text.len() {
            return false;
        }
        let Ok(end) = u32::try_from(replacement.len()) else {
            return false;
        };
        let Ok(old_scalars) = u32::try_from(text.chars().count()) else {
            return false;
        };
        let Ok(new_scalars) = u32::try_from(replacement.chars().count()) else {
            return false;
        };
        let root_key = atom.root;
        let link = atom.link;
        let Some(root) = self.store.root(root_key) else {
            return false;
        };
        let Some(atom_position) = root.atoms.iter().position(|key| *key == content.atom) else {
            return false;
        };
        let mut point_updates = Vec::new();
        for &point_key in &root.points {
            let Some(point) = self.store.point(point_key) else {
                return false;
            };
            let follows = match point.boundary {
                PointBoundary::BetweenAtoms { atom_boundary } => {
                    atom_boundary as usize > atom_position
                }
                PointBoundary::InAtom { atom, .. } if atom == content.atom => return false,
                PointBoundary::InAtom { atom, .. } => {
                    let Some(position) = root.atoms.iter().position(|key| *key == atom) else {
                        return false;
                    };
                    position > atom_position
                }
            };
            if follows {
                let scalar_boundary = if new_scalars >= old_scalars {
                    point.scalar_boundary.checked_add(new_scalars - old_scalars)
                } else {
                    point.scalar_boundary.checked_sub(old_scalars - new_scalars)
                };
                let Some(scalar_boundary) = scalar_boundary else {
                    return false;
                };
                point_updates.push((point_key, scalar_boundary));
            }
        }
        let Some(atom) = self
            .store
            .atoms
            .get_mut(content.atom.index().unwrap_or(usize::MAX))
            .filter(|record| record.key == content.atom)
        else {
            return false;
        };
        match &mut atom.kind {
            ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
                *text = replacement;
            }
            ContentAtomKind::BreakOpportunity {} | ContentAtomKind::HardBreak {} => return false,
        }
        content.bytes.end = end;
        if let Some(link) = link
            && let Some(record) = self.link_mut(link)
        {
            for part in &mut record.label {
                if let LinkLabelPart::Content { content: label } = part
                    && label.atom == content.atom
                    && label.bytes.start == 0
                {
                    label.bytes.end = end;
                }
            }
        }
        for (point, scalar_boundary) in point_updates {
            if let Some(record) = point
                .index()
                .and_then(|index| self.store.points.get_mut(index))
            {
                record.scalar_boundary = scalar_boundary;
            }
        }
        true
    }

    /// Add one content owner.
    #[must_use]
    pub fn push_owner(
        &mut self,
        kind: ContentOwnerKind,
        provenance: Provenance,
    ) -> ContentOwnerKey {
        let key = dense_key(self.store.owners.len(), ContentOwnerKey::new);
        self.store.owners.push(ContentOwner {
            key,
            kind,
            roots: Vec::new(),
            provenance,
        });
        key
    }

    /// Add one logical root and attach it to its owner.
    ///
    /// # Panics
    ///
    /// Panics when `owner` was not allocated by this builder.
    #[must_use]
    pub fn push_root(
        &mut self,
        owner: ContentOwnerKey,
        kind: ContentRootKind,
        provenance: Provenance,
    ) -> ContentRootKey {
        let key = dense_key(self.store.roots.len(), ContentRootKey::new);
        self.store
            .owners
            .get_mut(owner.index().expect("content owner key fits usize"))
            .filter(|record| record.key == owner)
            .expect("content root owner belongs to this builder")
            .roots
            .push(key);
        self.store.roots.push(ContentRoot {
            key,
            owner,
            kind,
            atoms: Vec::new(),
            points: Vec::new(),
            provenance,
        });
        key
    }

    /// Reserve one link occurrence before adding its linked atoms.
    ///
    /// # Panics
    ///
    /// Panics when `owner` was not allocated by this builder.
    #[must_use]
    pub fn push_link(
        &mut self,
        owner: ContentOwnerKey,
        target: LinkTarget,
        title: Option<String>,
        provenance: Provenance,
    ) -> LinkOccurrenceKey {
        assert!(
            self.store.owner(owner).is_some(),
            "link owner belongs to this builder"
        );
        let key = dense_key(self.store.links.len(), LinkOccurrenceKey::new);
        self.store.links.push(LinkOccurrence {
            key,
            owner,
            target,
            title,
            label: Vec::new(),
            provenance,
        });
        key
    }

    /// Append one text atom and return a reference covering all its bytes.
    ///
    /// # Panics
    ///
    /// Panics when `root` or `link` was not allocated by this builder.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn push_text(
        &mut self,
        root: ContentRootKey,
        text: String,
        display_override: Option<String>,
        style: ContentStyle,
        role: Option<ContentRole>,
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
    ) -> ContentRef {
        self.push_string_atom(
            root,
            text,
            display_override,
            style,
            role,
            link,
            provenance,
            false,
            false,
        )
    }

    /// Append one whitespace atom and return a reference covering all bytes.
    ///
    /// # Panics
    ///
    /// Panics when `root` or `link` was not allocated by this builder.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn push_whitespace(
        &mut self,
        root: ContentRootKey,
        text: String,
        display_override: Option<String>,
        breakable: bool,
        style: ContentStyle,
        role: Option<ContentRole>,
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
    ) -> ContentRef {
        self.push_string_atom(
            root,
            text,
            display_override,
            style,
            role,
            link,
            provenance,
            true,
            breakable,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn push_string_atom(
        &mut self,
        root: ContentRootKey,
        text: String,
        display_override: Option<String>,
        style: ContentStyle,
        role: Option<ContentRole>,
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
        whitespace: bool,
        breakable: bool,
    ) -> ContentRef {
        let byte_len = u32::try_from(text.len()).expect("one content atom fits u32 bytes");
        let kind = if whitespace {
            ContentAtomKind::Whitespace {
                text,
                display_override,
                breakable,
            }
        } else {
            ContentAtomKind::Text {
                text,
                display_override,
            }
        };
        let atom = self.push_atom(root, kind, style, role, link, provenance);
        let content = ContentRef {
            atom,
            bytes: ContentByteRange {
                start: 0,
                end: byte_len,
            },
        };
        if let Some(link) = link {
            self.store
                .links
                .get_mut(link.index().expect("link key fits usize"))
                .filter(|record| record.key == link)
                .expect("linked atom occurrence belongs to this builder")
                .label
                .push(LinkLabelPart::Content { content });
        }
        content
    }

    /// Append one zero-width break opportunity.
    #[must_use]
    pub fn push_break_opportunity(
        &mut self,
        root: ContentRootKey,
        provenance: Provenance,
    ) -> ContentAtomKey {
        self.push_break_opportunity_with_metadata(root, ContentStyle::default(), None, provenance)
    }

    /// Append one zero-width break opportunity with retained native metadata.
    #[must_use]
    pub fn push_break_opportunity_with_metadata(
        &mut self,
        root: ContentRootKey,
        style: ContentStyle,
        role: Option<ContentRole>,
        provenance: Provenance,
    ) -> ContentAtomKey {
        self.push_atom(
            root,
            ContentAtomKind::BreakOpportunity {},
            style,
            role,
            None,
            provenance,
        )
    }

    /// Append one logical hard break.
    #[must_use]
    pub fn push_hard_break(
        &mut self,
        root: ContentRootKey,
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
    ) -> ContentAtomKey {
        self.push_hard_break_with_metadata(root, ContentStyle::default(), None, link, provenance)
    }

    /// Append one logical hard break with retained native metadata.
    ///
    /// # Panics
    ///
    /// Panics when `root` or `link` was not allocated by this builder.
    #[must_use]
    pub fn push_hard_break_with_metadata(
        &mut self,
        root: ContentRootKey,
        style: ContentStyle,
        role: Option<ContentRole>,
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
    ) -> ContentAtomKey {
        let atom = self.push_atom(
            root,
            ContentAtomKind::HardBreak {},
            style,
            role,
            link,
            provenance,
        );
        if let Some(link) = link {
            self.store
                .links
                .get_mut(link.index().expect("link key fits usize"))
                .filter(|record| record.key == link)
                .expect("linked break occurrence belongs to this builder")
                .label
                .push(LinkLabelPart::HardBreak { atom });
        }
        atom
    }

    fn push_atom(
        &mut self,
        root: ContentRootKey,
        kind: ContentAtomKind,
        style: ContentStyle,
        role: Option<ContentRole>,
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
    ) -> ContentAtomKey {
        let owner = self
            .store
            .root(root)
            .expect("content atom root belongs to this builder")
            .owner;
        if let Some(link) = link {
            assert!(
                self.store.link(link).is_some(),
                "atom link belongs to this builder"
            );
        }
        let key = dense_key(self.store.atoms.len(), ContentAtomKey::new);
        self.store.atoms.push(ContentAtom {
            key,
            root,
            owner,
            kind,
            style,
            role,
            link,
            provenance,
        });
        self.store
            .roots
            .get_mut(root.index().expect("content root key fits usize"))
            .filter(|record| record.key == root)
            .expect("content atom root belongs to this builder")
            .atoms
            .push(key);
        key
    }

    /// Append one zero-width point and attach it to its root.
    ///
    /// # Panics
    ///
    /// Panics when `root` was not allocated by this builder.
    #[must_use]
    pub fn push_point(
        &mut self,
        root: ContentRootKey,
        boundary: PointBoundary,
        scalar_boundary: u32,
        provenance: Provenance,
    ) -> ContentPointKey {
        let owner = self
            .store
            .root(root)
            .expect("content point root belongs to this builder")
            .owner;
        let key = dense_key(self.store.points.len(), ContentPointKey::new);
        self.store.points.push(ContentPoint {
            key,
            root,
            owner,
            boundary,
            scalar_boundary,
            provenance,
        });
        self.store
            .roots
            .get_mut(root.index().expect("content root key fits usize"))
            .filter(|record| record.key == root)
            .expect("content point root belongs to this builder")
            .points
            .push(key);
        key
    }

    /// Finish construction and return the owned store.
    #[must_use]
    pub fn finish(self) -> ContentStore {
        self.store
    }
}

pub(super) fn dense_key<K>(length: usize, constructor: impl FnOnce(u32) -> Option<K>) -> K {
    let value = u32::try_from(length)
        .ok()
        .and_then(|length| length.checked_add(1))
        .and_then(constructor);
    value.expect("content store exceeds u32 dense-key capacity")
}
