//! One dense transfer from native structured content into the public store.

mod fixed;

use libmandoc_rs::structured::{
    ContentAtomKey as NativeAtomKey, ContentAtomKind as NativeAtomKind,
    ContentOwnerKind as NativeOwnerKind, ContentPointKey as NativePointKey,
    ContentRootKey as NativeRootKey, ContentRootKind as NativeRootKind,
    LinkOccurrenceKey as NativeLinkKey, NativeLinkTarget, NativeRole,
    PointBoundary as NativeBoundary,
};
use mant_ir::{
    ContentAtomKey, ContentOwnerKind, ContentPointKey, ContentRole, ContentRootKey,
    ContentRootKind, ContentStore, ContentStoreBuilder, ContentStyle, FixedViewKey,
    LinkOccurrenceKey, LinkTarget, PointBoundary, Provenance, validate_content_store,
};

use super::{
    NativeProjectionError, NativeProseProjection,
    address::{AddressPlan, LinkAssignment},
};

/// Dense native-to-public keys plus the one authoritative owned store.
pub(super) struct NativeContentMap {
    store: ContentStore,
    roots: Vec<ContentRootKey>,
    native_roots: Vec<NativeRootKey>,
    atoms: Vec<ContentAtomKey>,
    native_atoms: Vec<NativeAtomKey>,
    points: Vec<ContentPointKey>,
    native_points: Vec<NativePointKey>,
    links: Vec<Option<LinkOccurrenceKey>>,
    fixed_views: Vec<FixedViewKey>,
}

impl NativeContentMap {
    #[allow(clippy::too_many_lines)] // Keep the native-to-public transfer and validation together.
    pub(super) fn build(
        projection: &mut NativeProseProjection,
        addresses: &AddressPlan,
    ) -> Result<Self, NativeProjectionError> {
        let provenances = projection.provenances().to_vec();
        let fixed_tables = projection.take_fixed_tables();
        let tables = projection.take_content_tables();
        let (owners, roots, atoms, points, links) = tables.into_parts();
        let mut builder = ContentStoreBuilder::new();

        let mut owner_keys = Vec::new();
        reserve(
            &mut owner_keys,
            owners.len(),
            "content owner key map allocation",
        )?;
        for owner in &owners {
            owner_keys.push(builder.push_owner(
                owner_kind(owner.kind()),
                provenance(&provenances, owner.provenance())?,
            ));
        }

        let mut root_keys = Vec::new();
        let mut native_root_keys = Vec::new();
        reserve(
            &mut root_keys,
            roots.len(),
            "content root key map allocation",
        )?;
        reserve(
            &mut native_root_keys,
            roots.len(),
            "reverse content root key map allocation",
        )?;
        for root in &roots {
            root_keys.push(builder.push_root(
                mapped(
                    &owner_keys,
                    root.owner().get(),
                    "content root owner is unknown",
                )?,
                root_kind(root.kind()),
                provenance(&provenances, root.provenance())?,
            ));
            native_root_keys.push(root.key());
        }

        let mut link_keys = Vec::new();
        reserve(
            &mut link_keys,
            links.len(),
            "content link key map allocation",
        )?;
        for link in links {
            let native_key = link.key();
            let owner = mapped(
                &owner_keys,
                link.owner().get(),
                "content link owner is unknown",
            )?;
            let link_provenance = provenance(&provenances, link.provenance())?;
            let assignment = addresses.link_assignment(native_key)?;
            let (native_target, title) = link.into_target_and_title();
            let target = match assignment {
                LinkAssignment::Native => Some(native_target_into_public(native_target)?),
                LinkAssignment::Section(id) => Some(LinkTarget::Section { id: id.clone() }),
                LinkAssignment::Dropped => None,
            };
            link_keys.push(
                target.map(|target| builder.push_link(owner, target, title, link_provenance)),
            );
        }

        let mut atom_keys = Vec::new();
        let mut native_atom_keys = Vec::new();
        reserve(
            &mut atom_keys,
            atoms.len(),
            "content atom key map allocation",
        )?;
        reserve(
            &mut native_atom_keys,
            atoms.len(),
            "reverse content atom key map allocation",
        )?;
        for atom in atoms {
            native_atom_keys.push(atom.key());
            let root = mapped(
                &root_keys,
                atom.root().get(),
                "content atom root is unknown",
            )?;
            let link = atom
                .link()
                .map(|key| mapped(&link_keys, key.get(), "content atom link is unknown"))
                .transpose()?
                .flatten();
            let style = style(atom.style());
            let role = atom.role().map(role);
            let atom_provenance = provenance(&provenances, atom.provenance())?;
            let key = match atom.into_kind() {
                NativeAtomKind::Text {
                    text,
                    display_override,
                } => {
                    let content = builder.push_text(
                        root,
                        text,
                        display_override,
                        style,
                        role,
                        link,
                        atom_provenance,
                    );
                    content.atom
                }
                NativeAtomKind::Whitespace {
                    text,
                    display_override,
                    breakable,
                } => {
                    let content = builder.push_whitespace(
                        root,
                        text,
                        display_override,
                        breakable,
                        style,
                        role,
                        link,
                        atom_provenance,
                    );
                    content.atom
                }
                NativeAtomKind::BreakOpportunity => {
                    builder.push_break_opportunity_with_metadata(root, style, role, atom_provenance)
                }
                NativeAtomKind::HardBreak => {
                    builder.push_hard_break_with_metadata(root, style, role, link, atom_provenance)
                }
            };
            atom_keys.push(key);
        }

        let mut point_keys = Vec::new();
        let mut native_point_keys = Vec::new();
        reserve(
            &mut point_keys,
            points.len(),
            "content point key map allocation",
        )?;
        reserve(
            &mut native_point_keys,
            points.len(),
            "reverse content point key map allocation",
        )?;
        for point in &points {
            native_point_keys.push(point.key());
            let root = mapped(
                &root_keys,
                point.root().get(),
                "content point root is unknown",
            )?;
            let boundary = match point.boundary() {
                NativeBoundary::BetweenAtoms { atom_boundary } => PointBoundary::BetweenAtoms {
                    atom_boundary: *atom_boundary,
                },
                NativeBoundary::InAtom { atom, byte_offset } => PointBoundary::InAtom {
                    atom: mapped(&atom_keys, atom.get(), "content point atom is unknown")?,
                    byte_offset: *byte_offset,
                },
            };
            point_keys.push(builder.push_point(
                root,
                boundary,
                point.scalar_boundary(),
                provenance(&provenances, point.provenance())?,
            ));
        }

        let mut store = builder.finish();
        let fixed_views = fixed::transfer_fixed(
            &mut store,
            fixed_tables,
            &owner_keys,
            &atom_keys,
            &point_keys,
            &provenances,
        )?;
        validate_content_store(&store).map_err(|_| {
            NativeProjectionError::InvalidRelation("native content store transfer is invalid")
        })?;
        Ok(Self {
            store,
            roots: root_keys,
            native_roots: native_root_keys,
            atoms: atom_keys,
            native_atoms: native_atom_keys,
            points: point_keys,
            native_points: native_point_keys,
            links: link_keys,
            fixed_views,
        })
    }

    pub(super) fn root(
        &self,
        key: libmandoc_rs::structured::ContentRootKey,
    ) -> Result<ContentRootKey, NativeProjectionError> {
        mapped(
            &self.roots,
            key.get(),
            "native root has no public content key",
        )
    }

    pub(super) fn atom(
        &self,
        key: libmandoc_rs::structured::ContentAtomKey,
    ) -> Result<ContentAtomKey, NativeProjectionError> {
        mapped(
            &self.atoms,
            key.get(),
            "native atom has no public content key",
        )
    }

    pub(super) fn native_root(
        &self,
        key: ContentRootKey,
    ) -> Result<NativeRootKey, NativeProjectionError> {
        mapped(
            &self.native_roots,
            key.get(),
            "public root has no native content key",
        )
    }

    pub(super) fn native_atom(
        &self,
        key: ContentAtomKey,
    ) -> Result<NativeAtomKey, NativeProjectionError> {
        mapped(
            &self.native_atoms,
            key.get(),
            "public atom has no native content key",
        )
    }

    pub(super) fn native_point(
        &self,
        key: ContentPointKey,
    ) -> Result<NativePointKey, NativeProjectionError> {
        mapped(
            &self.native_points,
            key.get(),
            "public point has no native content key",
        )
    }

    pub(super) fn point(
        &self,
        key: NativePointKey,
    ) -> Result<ContentPointKey, NativeProjectionError> {
        mapped(
            &self.points,
            key.get(),
            "native point has no public content key",
        )
    }

    pub(super) fn link(
        &self,
        key: NativeLinkKey,
    ) -> Result<Option<LinkOccurrenceKey>, NativeProjectionError> {
        mapped(
            &self.links,
            key.get(),
            "native link has no public occurrence key",
        )
    }

    pub(super) fn fixed_view(
        &self,
        key: libmandoc_rs::structured::NativeFixedViewKey,
    ) -> Result<FixedViewKey, NativeProjectionError> {
        mapped(
            &self.fixed_views,
            key.get(),
            "native fixed view has no public key",
        )
    }

    pub(super) const fn store(&self) -> &ContentStore {
        &self.store
    }

    pub(super) fn into_store(self) -> ContentStore {
        self.store
    }
}

fn native_target_into_public(
    target: NativeLinkTarget,
) -> Result<LinkTarget, NativeProjectionError> {
    Ok(match target {
        NativeLinkTarget::External(uri) => LinkTarget::External { uri },
        NativeLinkTarget::Email(address) => LinkTarget::Email { address },
        NativeLinkTarget::Document(name) => LinkTarget::Document {
            name,
            fragment: None,
        },
        NativeLinkTarget::Manual { name, section } => LinkTarget::Manual {
            name,
            manual_section: Some(section),
        },
        NativeLinkTarget::Section(_) => {
            return Err(NativeProjectionError::InvalidRelation(
                "resolved section link retained its native phrase",
            ));
        }
    })
}

const fn owner_kind(kind: NativeOwnerKind) -> ContentOwnerKind {
    match kind {
        NativeOwnerKind::Document => ContentOwnerKind::Document,
        NativeOwnerKind::Section => ContentOwnerKind::Section,
        NativeOwnerKind::ListItem => ContentOwnerKind::ListItem,
        NativeOwnerKind::DefinitionItem => ContentOwnerKind::DefinitionItem,
        NativeOwnerKind::Paragraph | NativeOwnerKind::FixedDisplay => ContentOwnerKind::Content,
        NativeOwnerKind::TableCell => ContentOwnerKind::TableCell,
    }
}

const fn root_kind(kind: NativeRootKind) -> ContentRootKind {
    match kind {
        NativeRootKind::Heading => ContentRootKind::Heading,
        NativeRootKind::Term => ContentRootKind::Term,
        NativeRootKind::Body => ContentRootKind::Body,
        NativeRootKind::Cell => ContentRootKind::Cell,
        NativeRootKind::FixedBody => ContentRootKind::FixedBody,
    }
}

const fn style(style: libmandoc_rs::structured::StructuredStyle) -> ContentStyle {
    ContentStyle {
        strong: style.is_bold(),
        emphasis: style.is_italic(),
        literal: style.is_literal(),
        underline: style.is_underline(),
    }
}

const fn role(role: NativeRole) -> ContentRole {
    match role {
        NativeRole::Flag => ContentRole::Flag,
        NativeRole::EnvironmentVariable => ContentRole::EnvironmentVariable,
        NativeRole::Argument => ContentRole::Argument,
        NativeRole::CommandOrDirective => ContentRole::CommandOrDirective,
        NativeRole::Path => ContentRole::Path,
    }
}

fn provenance(
    values: &[Provenance],
    key: libmandoc_rs::structured::ProvenanceKey,
) -> Result<Provenance, NativeProjectionError> {
    mapped(values, key.get(), "native content provenance is unknown")
}

fn mapped<T: Copy>(
    values: &[T],
    key: u32,
    relation: &'static str,
) -> Result<T, NativeProjectionError> {
    key.checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| values.get(index))
        .copied()
        .ok_or(NativeProjectionError::InvalidRelation(relation))
}

fn reserve<T>(
    values: &mut Vec<T>,
    additional: usize,
    relation: &'static str,
) -> Result<(), NativeProjectionError> {
    values
        .try_reserve_exact(additional)
        .map_err(|_| NativeProjectionError::InvalidRelation(relation))
}
