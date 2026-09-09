//! Optional navigation facts emitted by the same authoritative content walk.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{LinkOccurrenceRef, ReferenceOwnerRef};
use crate::{
    ContentLocation, ContentLocationRef, DocumentReference, EntryOwnerLocationRef, FragmentAlias,
    LinkTarget, NodeId, SourceSpan,
};

/// Kind of the original typed link, before any destination lookup.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceTargetType {
    /// Registered Markdown document reference.
    Document,
    /// Native manual reference, with or without a section.
    Manual,
    /// Same-document content identity or authored fragment.
    Local,
    /// External URI; no existence probing is implied.
    External,
    /// Email address; no existence probing is implied.
    Email,
}

impl ReferenceTargetType {
    /// Classify without cloning or inspecting a target's strings.
    #[must_use]
    pub const fn of(target: &LinkTarget) -> Self {
        match target {
            LinkTarget::Document { .. } => Self::Document,
            LinkTarget::Manual { .. } => Self::Manual,
            LinkTarget::Section { .. } => Self::Local,
            LinkTarget::External { .. } => Self::External,
            LinkTarget::Email { .. } => Self::Email,
        }
    }
}

/// Allocation-free link selection, applied before inspecting target strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceLinkFilter(u8);

impl ReferenceLinkFilter {
    /// No visible-link callbacks or target-string inspection.
    pub const NONE: Self = Self(0);
    /// Every original typed link.
    pub const ALL: Self = Self(31);
    /// Markdown and manual targets, the default outline discovery policy.
    pub const DOCUMENTS: Self = Self(3);

    /// Select these types; duplicates have no additional effect.
    #[must_use]
    pub fn from_types(types: &[ReferenceTargetType]) -> Self {
        Self(types.iter().fold(0, |bits, kind| bits | (1 << *kind as u8)))
    }

    /// Whether the original target kind is selected.
    #[must_use]
    pub const fn contains(self, target: &LinkTarget) -> bool {
        self.0 & (1 << ReferenceTargetType::of(target) as u8) != 0
    }
}

/// Requested event families. Unrequested alias/domain metadata is not inspected.
#[derive(Debug, Clone, Copy)]
pub struct NavigationScanOptions {
    /// Visible target types to visit.
    pub links: ReferenceLinkFilter,
    /// Emit canonical destinations and authored fragment aliases.
    pub targets: bool,
    /// Emit independent semantic entry-set relations.
    pub entry_sets: bool,
}

impl Default for NavigationScanOptions {
    fn default() -> Self {
        Self {
            links: ReferenceLinkFilter::ALL,
            targets: false,
            entry_sets: false,
        }
    }
}

/// Exact destination in an already loaded document, not a readable selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ContentReveal {
    /// Document origin, including heading-only or alias-only documents.
    Document {},
    /// Section start, independent of semantic entries.
    Section {
        /// Zero-based section path.
        sections: Vec<u32>,
    },
    /// Original list/definition item; not its remote document target.
    Owner {
        /// Zero-based section path.
        sections: Vec<u32>,
        /// Exact containing list/definition-list path.
        blocks: Vec<crate::ContentBlockStep>,
        /// Zero-based item index.
        item_index: u32,
    },
    /// Exact inline destination, including zero-width anchors.
    Inline {
        /// Snapshot-local inline node position.
        location: ContentLocation,
    },
}

/// Borrowed counterpart, valid only during a navigation callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRevealRef<'a> {
    /// Document origin.
    Document,
    /// Section start.
    Section(&'a [u32]),
    /// Original item.
    Owner(EntryOwnerLocationRef<'a>),
    /// Exact inline node.
    Inline(ContentLocationRef<'a>),
}

impl ContentReveal {
    /// Borrow an exact destination without allocating another coordinate path.
    #[must_use]
    pub fn as_ref(&self) -> ContentRevealRef<'_> {
        match self {
            Self::Document {} => ContentRevealRef::Document,
            Self::Section { sections } => ContentRevealRef::Section(sections),
            Self::Owner {
                sections,
                blocks,
                item_index,
            } => ContentRevealRef::Owner(EntryOwnerLocationRef {
                sections,
                blocks,
                item_index: *item_index,
            }),
            Self::Inline { location } => ContentRevealRef::Inline(location.as_ref()),
        }
    }
}

impl ContentRevealRef<'_> {
    /// Number of coordinates inspected or retained by this destination.
    #[must_use]
    pub fn depth(self) -> usize {
        match self {
            Self::Document => 0,
            Self::Section(sections) => sections.len(),
            Self::Owner(owner) => owner
                .sections
                .len()
                .saturating_add(owner.blocks.len())
                .saturating_add(1),
            Self::Inline(location) => location.depth(),
        }
    }

    /// Exact compact encoded size, checked before allocation.
    #[must_use]
    pub fn encoded_size_bound(self) -> usize {
        match self {
            Self::Document => r#"{"kind":"document"}"#.len(),
            Self::Section(sections) => {
                ContentLocationRef::SectionHeading {
                    sections,
                    path: &[],
                }
                .encoded_len()
                    - "-heading".len()
                    - ",\"path\":[]".len()
            }
            Self::Owner(owner) => {
                let raw = ContentLocationRef::Content {
                    sections: owner.sections,
                    blocks: owner.blocks,
                    root: crate::ContentInlineRoot::Inlines,
                    path: &[],
                }
                .encoded_len();
                raw - "content".len() + "owner".len()
                    - ",\"root\":{\"kind\":\"inlines\"},\"path\":[]".len()
                    + ",\"itemIndex\":".len()
                    + owner
                        .item_index
                        .checked_ilog10()
                        .map_or(1, |n| n as usize + 1)
            }
            Self::Inline(location) => location
                .encoded_len()
                .saturating_add(r#"{"kind":"inline","location":}"#.len()),
        }
    }

    /// Retain a bounded destination. Callers charge copy work/materialization first.
    #[must_use]
    pub fn to_owned(self) -> Option<ContentReveal> {
        if self.depth() > crate::MAX_CONTENT_DEPTH
            || self.encoded_size_bound() > crate::MAX_CONTENT_LOCATION_BYTES
        {
            return None;
        }
        Some(match self {
            Self::Document => ContentReveal::Document {},
            Self::Section(sections) => ContentReveal::Section {
                sections: sections.to_vec(),
            },
            Self::Owner(owner) => ContentReveal::Owner {
                sections: owner.sections.to_vec(),
                blocks: owner.blocks.to_vec(),
                item_index: owner.item_index,
            },
            Self::Inline(location) => ContentReveal::Inline {
                location: location.to_owned()?,
            },
        })
    }
}

/// One logical destination and all explicitly authored aliases at that location.
#[derive(Debug, Clone, Copy)]
pub struct NavigationTargetRef<'ir, 'path> {
    /// Exact canonical identity; never normalized again during navigation.
    pub id: &'ir NodeId,
    /// Authored spellings, inspected only when target events are requested.
    pub aliases: &'ir [FragmentAlias],
    /// Exact destination in this loaded IR snapshot.
    pub reveal: ContentRevealRef<'path>,
}

/// Independent entry-set relation, never counted as a visible occurrence.
#[derive(Debug, Clone, Copy)]
pub struct EntrySetReferenceRef<'ir, 'path> {
    /// Original restricted document/manual reference.
    pub reference: &'ir DocumentReference,
    /// Actual declaring item, not a form copy.
    pub owner: ReferenceOwnerRef<'ir, 'path>,
    /// Declaration provenance for source-order merging.
    pub source: Option<SourceSpan>,
}

/// Optional facts from one bounded DFS over authoritative content.
#[derive(Debug, Clone, Copy)]
pub enum NavigationEvent<'ir, 'path> {
    /// One selected real `Inline::Link`.
    Link(LinkOccurrenceRef<'ir, 'path>),
    /// A local reveal destination; no catalog lookup is performed.
    Target(NavigationTargetRef<'ir, 'path>),
    /// An independent semantic relation.
    EntrySet(EntrySetReferenceRef<'ir, 'path>),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reveal_sizes_match_closed_wire_and_do_not_reject_deep_sections() {
        let sections = vec![u32::MAX; 200];
        let blocks = [crate::ContentBlockStep::Block { index: 123 }];
        for reveal in [
            ContentRevealRef::Document,
            ContentRevealRef::Section(&sections),
            ContentRevealRef::Owner(EntryOwnerLocationRef {
                sections: &[2],
                blocks: &blocks,
                item_index: 12,
            }),
            ContentRevealRef::Inline(ContentLocationRef::DocumentHeading { path: &[1] }),
        ] {
            let owned = reveal.to_owned().unwrap();
            assert_eq!(
                reveal.encoded_size_bound(),
                serde_json::to_vec(&owned).unwrap().len()
            );
        }
    }
}
