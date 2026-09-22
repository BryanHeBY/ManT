//! Authoritative logical inline content and document-local relation keys.
//!
//! Structural [`crate::Inline`] nodes reference this store.  The store owns
//! visible text, typed link destinations, zero-width points, and the native
//! ownership/provenance relations needed to validate those references.

use std::num::NonZeroU32;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{LinkTarget, Provenance};

mod builder;
mod validation;

pub use builder::ContentStoreBuilder;
pub use validation::{ContentStoreError, validate_content_store};

use builder::{dense_key, detach_link};
use validation::validate_projection_limits;

macro_rules! content_key {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
            JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(NonZeroU32);

        impl $name {
            /// The first valid dense key.
            pub const FIRST: Self = Self(NonZeroU32::MIN);

            /// Construct a nonzero key.
            #[must_use]
            pub const fn new(value: u32) -> Option<Self> {
                match NonZeroU32::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            /// Return the one-based integer representation.
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0.get()
            }

            pub(crate) fn index(self) -> Option<usize> {
                usize::try_from(self.get() - 1).ok()
            }
        }
    };
}

content_key!(ContentOwnerKey, "Document-local content owner key.");
content_key!(ContentRootKey, "Document-local logical inline-root key.");
content_key!(ContentAtomKey, "Document-local logical atom key.");
content_key!(ContentPointKey, "Document-local zero-width point key.");
content_key!(
    LinkOccurrenceKey,
    "Document-local logical link occurrence key."
);

/// Half-open UTF-8 byte range in one text or whitespace atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentByteRange {
    /// Inclusive byte offset.
    pub start: u32,
    /// Exclusive byte offset.
    pub end: u32,
}

/// A checked slice of one authoritative logical atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentRef {
    /// Referenced text or whitespace atom.
    pub atom: ContentAtomKey,
    /// UTF-8 byte range in that atom's logical text.
    pub bytes: ContentByteRange,
}

/// Stable source-neutral owner category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ContentOwnerKind {
    /// Whole normalized document.
    Document,
    /// Section subtree.
    Section,
    /// Ordinary list item.
    ListItem,
    /// Definition-list item.
    DefinitionItem,
    /// Table cell.
    TableCell,
    /// Other producer-defined content boundary.
    Content,
}

/// One owner of roots and structural blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentOwner {
    /// Dense key equal to this record's position.
    pub key: ContentOwnerKey,
    /// Stable owner category.
    pub kind: ContentOwnerKind,
    /// Roots owned in logical source order.
    pub roots: Vec<ContentRootKey>,
    /// Authorship of the owner boundary.
    pub provenance: Provenance,
}

/// Stable logical-root category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ContentRootKind {
    /// Visible document or section heading.
    Heading,
    /// Ordinary flow body.
    Body,
    /// Definition or list term.
    Term,
    /// Table-cell body.
    Cell,
    /// Fixed-display logical body.
    FixedBody,
}

/// One independently searchable logical inline sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentRoot {
    /// Dense key equal to this record's position.
    pub key: ContentRootKey,
    /// Owning structural object.
    pub owner: ContentOwnerKey,
    /// Stable root category.
    pub kind: ContentRootKind,
    /// Atoms in authoritative logical order.
    pub atoms: Vec<ContentAtomKey>,
    /// Zero-width points in root order.
    pub points: Vec<ContentPointKey>,
    /// Authorship of this root.
    pub provenance: Provenance,
}

/// Native text attributes attached to one atom.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct ContentStyle {
    /// Strong/bold presentation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub strong: bool,
    /// Emphasized/italic presentation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub emphasis: bool,
    /// Literal/code presentation.
    #[serde(default, skip_serializing_if = "is_false")]
    pub literal: bool,
    /// Underline presentation retained from native output.
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
}

/// Optional native semantic role, distinct from inferred entry facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ContentRole {
    /// Command-line flag.
    Flag,
    /// Environment variable.
    EnvironmentVariable,
    /// Command argument or placeholder.
    Argument,
    /// Command or configuration directive.
    CommandOrDirective,
    /// Filesystem-like path.
    Path,
}

/// Logical atom payload.  Visual wrapping never creates another atom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ContentAtomKind {
    /// Visible non-whitespace text.
    Text {
        /// Searchable and copyable logical text.
        text: String,
        /// Optional profile-specific glyph projection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_override: Option<String>,
    },
    /// Visible logical whitespace.
    Whitespace {
        /// Searchable and copyable logical whitespace.
        text: String,
        /// Optional profile-specific glyph projection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_override: Option<String>,
        /// Whether ordinary flow may wrap after this atom.
        breakable: bool,
    },
    /// Zero-width legal wrapping boundary.
    BreakOpportunity {},
    /// One logical newline.
    HardBreak {},
}

impl ContentAtomKind {
    /// Return logical UTF-8 text when the atom owns bytes.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } | Self::Whitespace { text, .. } => Some(text),
            Self::BreakOpportunity {} | Self::HardBreak {} => None,
        }
    }
}

/// One atom in global retained execution order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentAtom {
    /// Dense key equal to this record's position.
    pub key: ContentAtomKey,
    /// Logical root containing this atom.
    pub root: ContentRootKey,
    /// Owner repeated for bounded validation.
    pub owner: ContentOwnerKey,
    /// Atom payload.
    #[serde(flatten)]
    pub kind: ContentAtomKind,
    /// Native style attributes.
    #[serde(default, skip_serializing_if = "ContentStyle::is_plain")]
    pub style: ContentStyle,
    /// Optional native semantic role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<ContentRole>,
    /// Logical link occurrence containing this atom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkOccurrenceKey>,
    /// Authorship of the atom.
    pub provenance: Provenance,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ContentAtomWire {
    Text {
        key: ContentAtomKey,
        root: ContentRootKey,
        owner: ContentOwnerKey,
        text: String,
        #[serde(default)]
        display_override: Option<String>,
        #[serde(default)]
        style: ContentStyle,
        #[serde(default)]
        role: Option<ContentRole>,
        #[serde(default)]
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
    },
    Whitespace {
        key: ContentAtomKey,
        root: ContentRootKey,
        owner: ContentOwnerKey,
        text: String,
        #[serde(default)]
        display_override: Option<String>,
        breakable: bool,
        #[serde(default)]
        style: ContentStyle,
        #[serde(default)]
        role: Option<ContentRole>,
        #[serde(default)]
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
    },
    BreakOpportunity {
        key: ContentAtomKey,
        root: ContentRootKey,
        owner: ContentOwnerKey,
        #[serde(default)]
        style: ContentStyle,
        #[serde(default)]
        role: Option<ContentRole>,
        #[serde(default)]
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
    },
    HardBreak {
        key: ContentAtomKey,
        root: ContentRootKey,
        owner: ContentOwnerKey,
        #[serde(default)]
        style: ContentStyle,
        #[serde(default)]
        role: Option<ContentRole>,
        #[serde(default)]
        link: Option<LinkOccurrenceKey>,
        provenance: Provenance,
    },
}

impl<'de> Deserialize<'de> for ContentAtom {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ContentAtomWire::deserialize(deserializer)?;
        let (key, root, owner, kind, style, role, link, provenance) = match wire {
            ContentAtomWire::Text {
                key,
                root,
                owner,
                text,
                display_override,
                style,
                role,
                link,
                provenance,
            } => (
                key,
                root,
                owner,
                ContentAtomKind::Text {
                    text,
                    display_override,
                },
                style,
                role,
                link,
                provenance,
            ),
            ContentAtomWire::Whitespace {
                key,
                root,
                owner,
                text,
                display_override,
                breakable,
                style,
                role,
                link,
                provenance,
            } => (
                key,
                root,
                owner,
                ContentAtomKind::Whitespace {
                    text,
                    display_override,
                    breakable,
                },
                style,
                role,
                link,
                provenance,
            ),
            ContentAtomWire::BreakOpportunity {
                key,
                root,
                owner,
                style,
                role,
                link,
                provenance,
            } => (
                key,
                root,
                owner,
                ContentAtomKind::BreakOpportunity {},
                style,
                role,
                link,
                provenance,
            ),
            ContentAtomWire::HardBreak {
                key,
                root,
                owner,
                style,
                role,
                link,
                provenance,
            } => (
                key,
                root,
                owner,
                ContentAtomKind::HardBreak {},
                style,
                role,
                link,
                provenance,
            ),
        };
        Ok(Self {
            key,
            root,
            owner,
            kind,
            style,
            role,
            link,
            provenance,
        })
    }
}

/// Exact zero-width position in one logical root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PointBoundary {
    /// Boundary between root atom slots.
    BetweenAtoms {
        /// Zero-based boundary in the root atom vector.
        atom_boundary: u32,
    },
    /// UTF-8 byte boundary inside a text or whitespace atom.
    InAtom {
        /// Atom containing the point.
        atom: ContentAtomKey,
        /// UTF-8 byte offset in the atom text.
        byte_offset: u32,
    },
}

/// One zero-width logical position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentPoint {
    /// Dense key equal to this record's position.
    pub key: ContentPointKey,
    /// Logical root containing the point.
    pub root: ContentRootKey,
    /// Owning structural object.
    pub owner: ContentOwnerKey,
    /// Structural atom boundary.
    #[serde(flatten)]
    pub boundary: PointBoundary,
    /// Derived root-relative Unicode-scalar boundary.
    pub scalar_boundary: u32,
    /// Authorship of the target position.
    pub provenance: Provenance,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum ContentPointWire {
    BetweenAtoms {
        key: ContentPointKey,
        root: ContentRootKey,
        owner: ContentOwnerKey,
        atom_boundary: u32,
        scalar_boundary: u32,
        provenance: Provenance,
    },
    InAtom {
        key: ContentPointKey,
        root: ContentRootKey,
        owner: ContentOwnerKey,
        atom: ContentAtomKey,
        byte_offset: u32,
        scalar_boundary: u32,
        provenance: Provenance,
    },
}

impl<'de> Deserialize<'de> for ContentPoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ContentPointWire::deserialize(deserializer)?;
        Ok(match wire {
            ContentPointWire::BetweenAtoms {
                key,
                root,
                owner,
                atom_boundary,
                scalar_boundary,
                provenance,
            } => Self {
                key,
                root,
                owner,
                boundary: PointBoundary::BetweenAtoms { atom_boundary },
                scalar_boundary,
                provenance,
            },
            ContentPointWire::InAtom {
                key,
                root,
                owner,
                atom,
                byte_offset,
                scalar_boundary,
                provenance,
            } => Self {
                key,
                root,
                owner,
                boundary: PointBoundary::InAtom { atom, byte_offset },
                scalar_boundary,
                provenance,
            },
        })
    }
}

/// One ordered component of a complete logical link label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LinkLabelPart {
    /// Text or whitespace content.
    Content {
        /// Exact atom slice.
        content: ContentRef,
    },
    /// One linked logical newline.
    HardBreak {
        /// Hard-break atom.
        atom: ContentAtomKey,
    },
}

/// One logical link occurrence, possibly represented by several wrappers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkOccurrence {
    /// Dense key equal to this record's position.
    pub key: LinkOccurrenceKey,
    /// Owning structural object.
    pub owner: ContentOwnerKey,
    /// Typed destination shared by every display fragment.
    pub target: LinkTarget,
    /// Optional non-visible advisory title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Complete ordered logical label, which may span roots.
    pub label: Vec<LinkLabelPart>,
    /// Authorship of the destination.
    pub provenance: Provenance,
}

/// The single authoritative content store for one document or projection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentStore {
    /// Structural owners in key order.
    pub owners: Vec<ContentOwner>,
    /// Logical roots in key order.
    pub roots: Vec<ContentRoot>,
    /// Logical atoms in global retained order.
    pub atoms: Vec<ContentAtom>,
    /// Zero-width points in key order.
    pub points: Vec<ContentPoint>,
    /// Logical link occurrences in key order.
    pub links: Vec<LinkOccurrence>,
}

impl ContentStore {
    /// Resolve an owner key.
    #[must_use]
    pub fn owner(&self, key: ContentOwnerKey) -> Option<&ContentOwner> {
        self.owners
            .get(key.index()?)
            .filter(|record| record.key == key)
    }

    /// Resolve a logical root key.
    #[must_use]
    pub fn root(&self, key: ContentRootKey) -> Option<&ContentRoot> {
        self.roots
            .get(key.index()?)
            .filter(|record| record.key == key)
    }

    /// Resolve an atom key.
    #[must_use]
    pub fn atom(&self, key: ContentAtomKey) -> Option<&ContentAtom> {
        self.atoms
            .get(key.index()?)
            .filter(|record| record.key == key)
    }

    /// Resolve a zero-width point key.
    #[must_use]
    pub fn point(&self, key: ContentPointKey) -> Option<&ContentPoint> {
        self.points
            .get(key.index()?)
            .filter(|record| record.key == key)
    }

    /// Resolve a logical link occurrence key.
    #[must_use]
    pub fn link(&self, key: LinkOccurrenceKey) -> Option<&LinkOccurrence> {
        self.links
            .get(key.index()?)
            .filter(|record| record.key == key)
    }

    /// Mutably resolve a link occurrence for a producer's final identity pass.
    #[must_use]
    pub fn link_mut(&mut self, key: LinkOccurrenceKey) -> Option<&mut LinkOccurrence> {
        self.links
            .get_mut(key.index()?)
            .filter(|record| record.key == key)
    }

    /// Detach one occurrence during a producer's final semantic pass.
    ///
    /// The occurrence retains its dense identity with an empty label while
    /// all former label atoms become unlinked. Invalid relations reject the
    /// operation before any mutation occurs.
    pub fn detach_link(&mut self, key: LinkOccurrenceKey) -> bool {
        detach_link(self, key)
    }

    /// Append a generated zero-width point during a producer's final identity pass.
    ///
    /// Structural producers normally create points through
    /// [`ContentStoreBuilder::push_point`].  Semantic identity allocation runs
    /// after all roots are known, however, and may need to attach a generated
    /// anchor to an existing root.  This is the only supported post-build
    /// mutation: it assigns the next dense key and updates root membership as
    /// one operation.
    ///
    /// # Panics
    ///
    /// Panics when `root` is not part of this store.
    #[must_use]
    pub fn append_generated_point(
        &mut self,
        root: ContentRootKey,
        boundary: PointBoundary,
        scalar_boundary: u32,
        trigger: Option<crate::SourceSpan>,
    ) -> ContentPointKey {
        let owner = self
            .root(root)
            .expect("generated content point root belongs to this store")
            .owner;
        let key = dense_key(self.points.len(), ContentPointKey::new);
        self.points.push(ContentPoint {
            key,
            root,
            owner,
            boundary,
            scalar_boundary,
            provenance: Provenance::Generated { trigger },
        });
        let insert_at = self
            .root(root)
            .expect("generated content point root belongs to this store")
            .points
            .partition_point(|point| {
                self.point(*point)
                    .is_some_and(|point| point.scalar_boundary <= scalar_boundary)
            });
        self.roots
            .get_mut(root.index().expect("content root key fits usize"))
            .filter(|record| record.key == root)
            .expect("generated content point root belongs to this store")
            .points
            .insert(insert_at, key);
        key
    }

    /// Resolve a checked content reference to its logical UTF-8 text.
    #[must_use]
    pub fn text(&self, content: ContentRef) -> Option<&str> {
        let text = self.atom(content.atom)?.kind.text()?;
        text.get(content.bytes.start as usize..content.bytes.end as usize)
    }

    /// Materialize one root's canonical logical sequence.
    ///
    /// Text and whitespace contribute their logical bytes, hard breaks
    /// contribute one newline, and break opportunities contribute nothing.
    /// Display overrides are deliberately ignored.
    #[must_use]
    pub fn root_logical_text(&self, key: ContentRootKey) -> Option<String> {
        let root = self.root(key)?;
        let mut output = String::new();
        for &atom_key in &root.atoms {
            let atom = self.atom(atom_key)?;
            if atom.root != key {
                return None;
            }
            match &atom.kind {
                ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
                    output.push_str(text);
                }
                ContentAtomKind::HardBreak {} => output.push('\n'),
                ContentAtomKind::BreakOpportunity {} => {}
            }
        }
        Some(output)
    }
}

impl ContentStyle {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    const fn is_plain(value: &Self) -> bool {
        !value.strong && !value.emphasis && !value.literal && !value.underline
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(value: &bool) -> bool {
    !*value
}

/// Response-local store for retained topology that does not embed a Document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentProjection {
    /// Complete closed store for all retained response-local references.
    pub content_store: ContentStore,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContentProjectionWire {
    content_store: ContentStore,
}

impl<'de> Deserialize<'de> for ContentProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ContentProjectionWire::deserialize(deserializer)?;
        validate_projection_limits(&wire.content_store).map_err(serde::de::Error::custom)?;
        validate_content_store(&wire.content_store).map_err(serde::de::Error::custom)?;
        Ok(Self {
            content_store: wire.content_store,
        })
    }
}

#[cfg(test)]
mod tests;
