//! Page-local original context, explicitly distinct from alias evidence.
use std::collections::HashSet;
use std::num::NonZeroU32;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One physical Fixed declaration, preserving its own final-display head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationFixedGroupMember {
    /// Exact one-based owner key in the queried Fixed document.
    pub key: NonZeroU32,
    /// Original semantic trail; grouping does not replace member identity.
    pub outline: crate::OutlineTrail,
    /// Complete final-display head, not a reconstructed Flow term.
    pub head: super::ExplanationFixedSelection,
}

/// Original content supporting directly matched declarations. IDs are indices
/// in the containing document response's pool, never persistent identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ExplanationSupport {
    /// A physical owner's original body reused by contained contexts.
    OwnedEntry {
        /// Single-entry original list excerpt, including nested content.
        block: mant_ir::Block,
    },
    /// Recovered adjacency supplies reading context, not proof that each
    /// sentence applies to every member or that members are interchangeable.
    DeclarationGroup {
        /// Exact containing list in the queried document snapshot.
        block_path: String,
        /// Half-open range in the original list, before response slicing.
        group: mant_ir::DeclarationGroup,
        /// Original members in source order; the last supplies the description.
        members: Vec<crate::OutlineTrail>,
        /// Complete original heads and final description. Local group indices
        /// are rebased; original item sources and identities remain unchanged.
        block: mant_ir::Block,
    },
    /// A nested declaration group already present in another returned source
    /// fragment. The descriptor preserves its own members and provider without
    /// copying their body again. References point directly to an owned fragment.
    ContainedDeclarationGroup {
        /// Original containing list in the document snapshot.
        block_path: String,
        /// Original member interval in the nested list.
        group: mant_ir::DeclarationGroup,
        /// Exact nested members, in source order.
        members: Vec<crate::OutlineTrail>,
        /// Index of a materialized `declaration-group` or `owned-entry`.
        support: usize,
        /// Typed path from that fragment's copied block to the nested list.
        path: Vec<super::ExplanationBlockStep>,
    },
    /// Native sibling declarations whose final member supplies reading context.
    /// This does not make the members aliases or inherit the provider's facts.
    FixedDeclarationGroup {
        /// Original independent owners in source order; the last is the provider.
        members: Vec<ExplanationFixedGroupMember>,
        /// Last member's original complete reading view; a returned provider may
        /// separately carry its own independent `fixed-owner` content.
        reading_body: super::ExplanationFixedSelection,
    },
}

impl ExplanationSupport {
    /// Validate the local/original ranges and exact member identities before
    /// accepting any reference. Context cannot silently point at another owner.
    #[must_use]
    pub fn items<'a>(
        &'a self,
        content: mant_ir::ContentContext<'a>,
    ) -> Option<&'a [mant_ir::DefinitionItem]> {
        let Self::DeclarationGroup {
            group,
            members,
            block,
            ..
        } = self
        else {
            return None;
        };
        let mant_ir::Block::DefinitionList {
            items,
            declaration_groups,
            ..
        } = block
        else {
            return None;
        };
        let width = group.end_item.checked_sub(group.start_item)?;
        let local = mant_ir::DeclarationGroup {
            start_item: 0,
            end_item: width,
        };
        if width > crate::MAX_EXPLANATION_RESULTS as usize
            || items.len() != width
            || members.len() != width
            || declaration_groups.as_slice() != [local]
            || items.iter().zip(members).any(|(item, member)| {
                mant_ir::EntryOwner::Definition(item)
                    .facts()
                    .is_none_or(|facts| facts.id.as_str() != member.node.id())
            })
        {
            return None;
        }
        local.resolve(content, items)
    }

    /// Resolve this group's original member interval, including a contained
    /// fragment. References never follow chains or cross document pools.
    #[must_use]
    pub fn items_in<'a>(
        &'a self,
        content: mant_ir::ContentContext<'a>,
        pool: &'a [Self],
    ) -> Option<&'a [mant_ir::DefinitionItem]> {
        let (block, group) = self.fragment(content, pool)?;
        let mant_ir::Block::DefinitionList { items, .. } = block else {
            return None;
        };
        group.resolve(content, items)
    }

    /// Member trails belonging to this group, not its enclosing context.
    #[must_use]
    pub fn members(&self) -> &[crate::OutlineTrail] {
        match self {
            Self::OwnedEntry { .. } | Self::FixedDeclarationGroup { .. } => &[],
            Self::DeclarationGroup { members, .. }
            | Self::ContainedDeclarationGroup { members, .. } => members,
        }
    }

    /// Materialized outer source block, if the complete descriptor is valid.
    /// Frontends render this block once and retain each group's own provenance.
    #[must_use]
    pub fn materialized<'a>(
        &'a self,
        content: mant_ir::ContentContext<'a>,
        pool: &'a [Self],
    ) -> Option<&'a mant_ir::Block> {
        if let Self::OwnedEntry { block } = self {
            return block.entry_owner().map(|_| block);
        }
        self.fragment(content, pool)?;
        match self {
            Self::OwnedEntry { .. } | Self::FixedDeclarationGroup { .. } => None,
            Self::DeclarationGroup { block, .. } => Some(block),
            Self::ContainedDeclarationGroup { support, .. } => match pool.get(*support)? {
                Self::DeclarationGroup { block, .. } | Self::OwnedEntry { block } => Some(block),
                Self::ContainedDeclarationGroup { .. } | Self::FixedDeclarationGroup { .. } => None,
            },
        }
    }

    fn fragment<'a>(
        &'a self,
        content: mant_ir::ContentContext<'a>,
        pool: &'a [Self],
    ) -> Option<(&'a mant_ir::Block, mant_ir::DeclarationGroup)> {
        match self {
            Self::OwnedEntry { .. } | Self::FixedDeclarationGroup { .. } => None,
            Self::DeclarationGroup { block, .. } => Some((
                block,
                mant_ir::DeclarationGroup {
                    start_item: 0,
                    end_item: self.items(content)?.len(),
                },
            )),
            Self::ContainedDeclarationGroup {
                group,
                members,
                support,
                path,
                ..
            } => {
                let parent = pool.get(*support)?;
                let block = match parent {
                    Self::DeclarationGroup { block, .. } => {
                        parent.items(content)?;
                        block
                    }
                    Self::OwnedEntry { block } => {
                        block.entry_owner()?;
                        block
                    }
                    Self::ContainedDeclarationGroup { .. } | Self::FixedDeclarationGroup { .. } => {
                        return None;
                    }
                };
                if path.is_empty() {
                    return None;
                }
                let block = super::locations::block_at(block, path)?;
                let mant_ir::Block::DefinitionList {
                    items,
                    declaration_groups,
                    ..
                } = block
                else {
                    return None;
                };
                let items = group.resolve(content, items)?;
                if !declaration_groups.contains(group)
                    || items.len() != members.len()
                    || items.len() > crate::MAX_EXPLANATION_RESULTS as usize
                    || items.iter().zip(members).any(|(item, member)| {
                        item.entry
                            .as_ref()
                            .is_none_or(|facts| facts.id.as_str() != member.node.id())
                    })
                {
                    return None;
                }
                Some((block, *group))
            }
        }
    }

    /// Check the detached Fixed context before accepting any evidence pointer.
    /// Native sibling ancestry and final-surface identity are producer checks;
    /// the wire still closes every local member and copied selection reference.
    fn valid_fixed_group(&self) -> bool {
        let Self::FixedDeclarationGroup {
            members,
            reading_body,
        } = self
        else {
            return false;
        };
        if !(2..=crate::MAX_EXPLANATION_RESULTS as usize).contains(&members.len())
            || reading_body.parts.is_empty()
            || reading_body.validate().is_err()
        {
            return false;
        }
        let mut keys = HashSet::with_capacity(members.len());
        let mut paths = HashSet::with_capacity(members.len());
        let mut ids = HashSet::with_capacity(members.len());
        members.iter().all(|member| {
            matches!(
                member.outline.node,
                crate::OutlineNodeReference::DocumentEntry { .. }
            ) && keys.insert(member.key)
                && paths.insert(member.outline.path())
                && ids.insert(member.outline.node.id())
                && member.outline.ancestors == members[0].outline.ancestors
                && !member.head.parts.is_empty()
                && member.head.validate().is_ok()
        })
    }
}

impl super::ExplanationContent {
    /// Resolve this source-qualified reference to its original physical owner.
    #[must_use]
    pub fn referenced_owner<'a>(
        &'a self,
        content: mant_ir::ContentContext<'a>,
        pool: &'a [ExplanationSupport],
    ) -> Option<mant_ir::EntryOwner<'a>> {
        if let Self::SharedEntry {
            support,
            path,
            item_index,
        } = self
        {
            let fragment = pool.get(*support)?;
            if matches!(
                fragment,
                ExplanationSupport::ContainedDeclarationGroup { .. }
            ) {
                return None;
            }
            let block = super::locations::block_at(fragment.materialized(content, pool)?, path)?;
            return match block {
                mant_ir::Block::DefinitionList { items, .. } => {
                    items.get(*item_index).map(mant_ir::EntryOwner::Definition)
                }
                mant_ir::Block::List { items, .. } => {
                    items.get(*item_index).map(mant_ir::EntryOwner::List)
                }
                _ => None,
            };
        }
        let Self::DeclarationMember {
            support,
            item_index,
        } = self
        else {
            return None;
        };
        Some(mant_ir::EntryOwner::Definition(
            pool.get(*support)?
                .items_in(content, pool)?
                .get(*item_index)?,
        ))
    }

    /// Resolve an owner-local position without copying the shared source body.
    #[must_use]
    pub fn resolve_range<'a>(
        &'a self,
        context: mant_ir::ContentContext<'a>,
        pool: &'a [ExplanationSupport],
        range: &super::ExplanationContentRange,
    ) -> Option<super::ExplanationTextRoot<'a>> {
        use super::{ExplanationBlockStep as Step, ExplanationContentRange as Range};
        match self {
            Self::FixedOwner { .. } => None,
            Self::SharedEntry {
                support,
                path,
                item_index,
            } => {
                self.referenced_owner(context, pool)?;
                remap_range(range, path, *item_index)?
                    .resolve(context, pool.get(*support)?.materialized(context, pool)?)
            }
            Self::Entry { block } | Self::Block { block } => range.resolve(context, block),
            Self::DeclarationMember {
                support,
                item_index,
            } => {
                self.referenced_owner(context, pool)?;
                let (block, group) = pool.get(*support)?.fragment(context, pool)?;
                let item_index = group.start_item.checked_add(*item_index)?;
                let mut mapped = range.clone();
                let path = match &mut mapped {
                    Range::DefinitionTerm {
                        path,
                        item_index: index,
                        ..
                    } if path.is_empty() => {
                        if *index != 0 {
                            return None;
                        }
                        *index = u32::try_from(item_index).ok()?;
                        return mapped.resolve(context, block);
                    }
                    Range::BlockText { path, .. } | Range::DefinitionTerm { path, .. } => path,
                };
                let Some(Step::DefinitionItem { index }) = path.first_mut() else {
                    return None;
                };
                if *index != 0 {
                    return None;
                }
                *index = u32::try_from(item_index).ok()?;
                mapped.resolve(context, block)
            }
        }
    }
}

impl super::ExplanationEvidence {
    /// Validated reference to returned original source, regardless of whether
    /// it also carries a declaration-group relationship.
    #[must_use]
    pub fn source_reference<'a>(
        &'a self,
        content: mant_ir::ContentContext<'a>,
        pool: &'a [ExplanationSupport],
    ) -> Option<usize> {
        if self.covered_by_support(content, pool) {
            self.support
        } else {
            self.shared_entry(content, pool)
        }
    }
    /// Validated physical-entry reference, distinct from a group relationship.
    #[must_use]
    pub fn shared_entry<'a>(
        &'a self,
        content: mant_ir::ContentContext<'a>,
        pool: &'a [ExplanationSupport],
    ) -> Option<usize> {
        let source @ super::ExplanationContent::SharedEntry { support, .. } =
            self.content.as_ref()?
        else {
            return None;
        };
        (self.class == super::EvidenceClass::DirectEntry
            && self.support.is_none()
            && !self.content_omitted
            && source
                .referenced_owner(content, pool)
                .is_some_and(|owner| self.matches_owner(content, owner)))
        .then_some(*support)
    }
    /// Whether a shared source fragment covers this exact owner and its forms.
    /// Invalid references must never hide separately returned metadata.
    #[must_use]
    pub fn covered_by_support<'a>(
        &'a self,
        content: mant_ir::ContentContext<'a>,
        pool: &'a [ExplanationSupport],
    ) -> bool {
        if matches!(
            self.content,
            Some(super::ExplanationContent::FixedOwner { .. })
        ) {
            return self.covered_by_fixed_support(pool);
        }
        let Some(source @ super::ExplanationContent::DeclarationMember { support, .. }) =
            &self.content
        else {
            return false;
        };
        self.support == Some(*support)
            && self.class == super::EvidenceClass::DirectEntry
            && !self.content_omitted
            && !self.support_omitted
            && source
                .referenced_owner(content, pool)
                .is_some_and(|owner| self.matches_owner(content, owner))
    }

    /// Check Fixed support without requiring a Flow content projection.
    #[must_use]
    pub fn covered_by_fixed_support(&self, pool: &[ExplanationSupport]) -> bool {
        if let Some(super::ExplanationContent::FixedOwner { key, reading_body }) = &self.content {
            let Some(
                support @ super::ExplanationSupport::FixedDeclarationGroup {
                    members,
                    reading_body: provider_body,
                },
            ) = self.support.and_then(|index| pool.get(index))
            else {
                return false;
            };
            if !support.valid_fixed_group() {
                return false;
            }
            let Some((index, member)) = members
                .iter()
                .enumerate()
                .find(|(_, member)| member.key == *key)
            else {
                return false;
            };
            return self.class == super::EvidenceClass::DirectEntry
                && !self.content_omitted
                && !self.support_omitted
                && member.outline == self.outline
                && self.entry.as_ref().is_none_or(|entry| {
                    entry.forms.is_empty()
                        && entry
                            .fixed_forms
                            .iter()
                            .all(|form| fixed_form_within_head(form, &member.head))
                })
                && if index + 1 == members.len() {
                    reading_body == provider_body
                } else {
                    reading_body.parts.is_empty()
                };
        }
        false
    }

    fn matches_owner(
        &self,
        content: mant_ir::ContentContext<'_>,
        owner: mant_ir::EntryOwner<'_>,
    ) -> bool {
        owner
            .facts()
            .is_some_and(|facts| facts.id.as_str() == self.outline.node.id())
            && content
                .entry_forms(owner)
                .ok()
                .flatten()
                .is_some_and(|forms| {
                    self.entry.as_ref().is_none_or(|entry| {
                        entry.forms.is_empty()
                            || forms.iter().eq(entry.forms.iter().map(Vec::as_slice))
                    })
                })
    }

    fn valid_references<'a>(
        &'a self,
        context: mant_ir::ContentContext<'a>,
        pool: &'a [ExplanationSupport],
    ) -> bool {
        use super::{EvidenceBasis, ExplanationContent, ExplanationOccurrence};
        if self
            .previews
            .len()
            .saturating_add(self.fixed_previews.len())
            > 2
            || !self.previews.is_empty() && !self.fixed_previews.is_empty()
            || self
                .fixed_previews
                .iter()
                .any(|preview| preview.validate().is_err())
            || !self.fixed_previews.is_empty()
                && (self.block_path.is_some()
                    || self
                        .entry
                        .as_ref()
                        .is_some_and(|entry| !entry.forms.is_empty())
                    || self.content.as_ref().is_some_and(|content| {
                        !matches!(content, ExplanationContent::FixedOwner { .. })
                    }))
            || !self.previews.is_empty()
                && matches!(self.content, Some(ExplanationContent::FixedOwner { .. }))
            || matches!(self.content, Some(ExplanationContent::SharedEntry { .. }))
                && self.shared_entry(context, pool).is_none()
            || self.support_omitted && self.class != super::EvidenceClass::DirectEntry
            || self.content_omitted && self.content.is_some()
            || self.support.is_some() && !self.covered_by_support(context, pool)
            || self.support.is_some_and(|index| pool.get(index).is_none())
            || self.support.is_some() && self.support_omitted
            || matches!(
                self.content,
                Some(ExplanationContent::DeclarationMember { .. })
            ) && !self.covered_by_support(context, pool)
            || self.entry.as_ref().is_some_and(|entry| {
                !entry.forms.is_empty() && !entry.fixed_forms.is_empty()
                    || entry
                        .fixed_forms
                        .iter()
                        .any(|form| form.validate().is_err())
            })
            || self.content.as_ref().is_some_and(|content| match content {
                ExplanationContent::FixedOwner { reading_body, .. } => {
                    self.class != super::EvidenceClass::DirectEntry
                        || self.support.is_some() && !self.covered_by_support(context, pool)
                        || reading_body.validate().is_err()
                        || self
                            .entry
                            .as_ref()
                            .is_some_and(|entry| !entry.forms.is_empty())
                }
                _ => self
                    .entry
                    .as_ref()
                    .is_some_and(|entry| !entry.fixed_forms.is_empty()),
            })
        {
            return false;
        }
        let valid_content = |range: &super::ExplanationContentRange| {
            self.content
                .as_ref()
                .is_some_and(|content| content.resolve_range(context, pool, range).is_some())
        };
        let valid_occurrence = |occurrence: &ExplanationOccurrence| {
            occurrence.forms.iter().all(|range| {
                self.entry
                    .as_ref()
                    .is_some_and(|entry| range.resolve(context, &entry.forms).is_some())
            }) && occurrence.content.iter().all(&valid_content)
                && (occurrence.fixed_forms.is_empty()
                    || occurrence.forms.is_empty() && occurrence.content.is_empty())
                && occurrence.fixed_forms.iter().all(|range| {
                    self.entry
                        .as_ref()
                        .is_some_and(|entry| range.resolve(&entry.fixed_forms).is_some())
                })
        };
        self.bases.iter().all(|basis| match basis {
            EvidenceBasis::Name { matches } => matches
                .iter()
                .all(|m| m.occurrences.iter().all(&valid_occurrence)),
            EvidenceBasis::Form { matches } => matches
                .iter()
                .all(|m| m.occurrences.iter().all(&valid_occurrence)),
            _ => true,
        }) && self.entry.as_ref().is_none_or(|entry| {
            entry.name_bindings.iter().all(|binding| {
                (binding.name_index as usize) < entry.names.len()
                    && binding.occurrences.iter().all(&valid_occurrence)
            })
        }) && self
            .previews
            .iter()
            .all(|p| p.content_ranges.iter().all(&valid_content))
    }
}

fn valid_pool<'a>(content: mant_ir::ContentContext<'a>, pool: &'a [ExplanationSupport]) -> bool {
    pool.len() <= crate::MAX_EXPLANATION_RESULTS as usize
        && pool.iter().all(|support| match support {
            ExplanationSupport::FixedDeclarationGroup { .. } => support.valid_fixed_group(),
            _ => support.materialized(content, pool).is_some(),
        })
}

/// Returned forms may be smaller than the complete native HEAD, but every
/// retained byte must still be an exact final-display byte of that HEAD.
fn fixed_form_within_head(
    form: &super::ExplanationFixedSelection,
    head: &super::ExplanationFixedSelection,
) -> bool {
    use unicode_width::UnicodeWidthStr;

    if form.parts.is_empty() || form.validate().is_err() {
        return false;
    }
    let mut cursor = 0;
    let mut prior: Option<usize> = None;
    let mut measured = None;
    for (part_index, part) in form.parts.iter().enumerate() {
        while head.parts.get(cursor).is_some_and(|head_part| {
            head_part.slice.run < part.slice.run
                || head_part.slice.run == part.slice.run
                    && head_part.slice.end_byte <= part.slice.start_byte
        }) {
            cursor += 1;
        }
        let Some(head_part) = head.parts.get(cursor) else {
            return false;
        };
        if part.slice.run != head_part.slice.run
            || part.slice.start_byte < head_part.slice.start_byte
            || part.slice.end_byte > head_part.slice.end_byte
            || part.row != head_part.row
            || part.run_column != head_part.run_column
            || part.style != head_part.style
            || part.source != head_part.source
        {
            return false;
        }
        let Some((start, end)) = part
            .slice
            .start_byte
            .checked_sub(head_part.slice.start_byte)
            .and_then(|start| {
                part.slice
                    .end_byte
                    .checked_sub(head_part.slice.start_byte)
                    .map(|end| (start, end))
            })
            .and_then(|(start, end)| {
                Some((usize::try_from(start).ok()?, usize::try_from(end).ok()?))
            })
        else {
            return false;
        };
        if head_part.text.get(start..end) != Some(part.text.as_str()) {
            return false;
        }
        if start == 0 && end == head_part.text.len() {
            if part.column != head_part.column || part.width != head_part.width {
                return false;
            }
        } else {
            let (measured_cursor, measured_end, measured_cells) =
                measured.unwrap_or((usize::MAX, 0usize, 0u32));
            let (prefix_end, prefix_cells) = if measured_cursor == cursor {
                (measured_end, measured_cells)
            } else {
                if u32::try_from(head_part.text.width()).ok() != Some(head_part.width) {
                    return false;
                }
                (0usize, 0u32)
            };
            let Some(gap) = head_part.text.get(prefix_end..start) else {
                return false;
            };
            let Some(cells) =
                prefix_cells.checked_add(u32::try_from(gap.width()).unwrap_or(u32::MAX))
            else {
                return false;
            };
            let Some(width) = u32::try_from(part.text.width()).ok() else {
                return false;
            };
            if head_part.column.checked_add(cells) != Some(part.column) || width != part.width {
                return false;
            }
            let Some(end_cells) = cells.checked_add(width) else {
                return false;
            };
            measured = Some((cursor, end, end_cells));
        }
        if let Some(previous) = prior {
            let Some(join) = form.joins.get(part_index - 1) else {
                return false;
            };
            if previous == cursor {
                if !matches!(join, mant_ir::TextJoin::DirectContact)
                    || form.parts[part_index - 1].slice.end_byte != part.slice.start_byte
                {
                    return false;
                }
            } else if previous + 1 != cursor || head.joins.get(previous) != Some(join) {
                return false;
            }
        }
        prior = Some(cursor);
    }
    true
}

fn remap_range(
    range: &super::ExplanationContentRange,
    prefix: &[super::ExplanationBlockStep],
    owner: usize,
) -> Option<super::ExplanationContentRange> {
    use super::{ExplanationBlockStep as Step, ExplanationContentRange as Range};
    let mut mapped = range.clone();
    let owner = u32::try_from(owner).ok()?;
    let path = match &mut mapped {
        Range::DefinitionTerm {
            path, item_index, ..
        } if path.is_empty() => {
            if *item_index != 0 {
                return None;
            }
            *item_index = owner;
            path.extend_from_slice(prefix);
            return Some(mapped);
        }
        Range::BlockText { path, .. } | Range::DefinitionTerm { path, .. } => path,
    };
    let (Step::DefinitionItem { index } | Step::ListItem { index }) = path.first_mut()? else {
        return None;
    };
    if *index != 0 {
        return None;
    }
    *index = owner;
    path.splice(0..0, prefix.iter().copied());
    Some(mapped)
}

impl super::QueryExplanation {
    /// Validate page-local source references and returned position domains.
    /// Deserialization runs this check; in-memory producers can call it too.
    ///
    /// # Errors
    /// Returns an error for dangling, wrong-owner or out-of-bounds references.
    pub fn validate_references(&self) -> Result<(), &'static str> {
        let fallback = mant_ir::ContentProjection {
            content_store: mant_ir::ContentStore::default(),
        };
        let content = self
            .content_projection
            .as_ref()
            .unwrap_or(&fallback)
            .content();
        (valid_pool(content, &self.supports)
            && self
                .evidence
                .iter()
                .all(|e| e.valid_references(content, &self.supports)))
        .then_some(())
        .ok_or("invalid explanation source reference or position")
    }
}

impl crate::ScopeExplanation {
    /// Validate references within their own document's pool, never another
    /// document with an equal-looking node ID.
    ///
    /// # Errors
    /// Returns an error for invalid document, support, owner or text positions.
    pub fn validate_references(&self) -> Result<(), &'static str> {
        (self.documents.iter().all(|document| {
            let fallback = mant_ir::ContentProjection {
                content_store: mant_ir::ContentStore::default(),
            };
            valid_pool(
                document
                    .content_projection
                    .as_ref()
                    .unwrap_or(&fallback)
                    .content(),
                &document.supports,
            )
        }) && self.evidence.iter().all(|evidence| {
            self.documents
                .get(evidence.document_index)
                .is_some_and(|document| {
                    let fallback = mant_ir::ContentProjection {
                        content_store: mant_ir::ContentStore::default(),
                    };
                    evidence.evidence.valid_references(
                        document
                            .content_projection
                            .as_ref()
                            .unwrap_or(&fallback)
                            .content(),
                        &document.supports,
                    )
                })
        }))
        .then_some(())
        .ok_or("invalid scoped explanation source reference or position")
    }
}

#[cfg(test)]
mod fixed_group_tests {
    use serde_json::{Value, json};

    use super::super::QueryExplanation;

    fn part(run: u32, row: u32, text: &str) -> Value {
        json!({
            "slice": {"run": run, "startByte": 0, "endByte": text.len()},
            "row": row,
            "runColumn": 0,
            "column": 0,
            "width": text.len(),
            "style": {"bold": false, "underline": false},
            "text": text
        })
    }

    fn selection(part: &Value) -> Value {
        json!({"parts": [part], "joins": []})
    }

    fn grouped_response() -> Value {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../../../tests/contracts/explanation-v0.12.json"
        ))
        .unwrap();
        let first = value["evidence"][0]["outline"].clone();
        let mut second = first.clone();
        second["node"]["path"] = "root/e2".into();
        second["node"]["id"] = "help-more".into();
        second["node"]["title"] = "--help-more".into();
        let first_head = selection(&part(1, 1, "--help"));
        let second_head = selection(&part(2, 2, "--help-more"));
        let body = selection(&part(3, 3, "description"));
        value["supports"] = json!([{
            "kind": "fixed-declaration-group",
            "members": [
                {"key": 1, "outline": first, "head": first_head},
                {"key": 2, "outline": second, "head": second_head}
            ],
            "readingBody": body
        }]);
        value["evidence"][0]["support"] = 0.into();
        value["evidence"][0]["contentOmitted"] = false.into();
        value["evidence"][0]["content"] = json!({
            "kind": "fixed-owner", "key": 1,
            "readingBody": {"parts": [], "joins": []}
        });
        value["evidence"][0]["entry"]["forms"] = json!([]);
        value["evidence"][0]["entry"]["fixedForms"] =
            json!([value["supports"][0]["members"][0]["head"].clone()]);
        for basis in value["evidence"][0]["bases"].as_array_mut().unwrap() {
            let occurrence = &mut basis["matches"][0]["occurrences"][0];
            occurrence["forms"] = json!([]);
            occurrence["fixedForms"] = json!([{"formIndex": 0, "startScalar": 0, "endScalar": 6}]);
        }
        let binding = &mut value["evidence"][0]["entry"]["nameBindings"][0]["occurrences"][0];
        binding["forms"] = json!([]);
        binding["fixedForms"] = json!([{"formIndex": 0, "startScalar": 0, "endScalar": 6}]);
        value
    }

    #[test]
    fn fixed_group_uses_exact_member_head_and_provider_body() {
        let original = grouped_response();
        let decoded: QueryExplanation = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(
            decoded.evidence[0].source_reference(
                decoded.content_projection.as_ref().unwrap().content(),
                &decoded.supports
            ),
            Some(0)
        );

        let mut duplicate_key = original.clone();
        duplicate_key["supports"][0]["members"][1]["key"] = 1.into();
        assert!(serde_json::from_value::<QueryExplanation>(duplicate_key).is_err());

        let mut singleton = original.clone();
        let first_member = singleton["supports"][0]["members"][0].clone();
        singleton["supports"][0]["members"] = json!([first_member]);
        assert!(serde_json::from_value::<QueryExplanation>(singleton).is_err());

        let mut foreign_member = original.clone();
        foreign_member["supports"][0]["members"][0]["outline"]["node"]["id"] = "foreign".into();
        assert!(serde_json::from_value::<QueryExplanation>(foreign_member).is_err());

        let mut foreign_form = original.clone();
        foreign_form["evidence"][0]["entry"]["fixedForms"][0]["parts"][0]["slice"]["run"] =
            9.into();
        assert!(serde_json::from_value::<QueryExplanation>(foreign_form).is_err());

        let mut foreign_source = original.clone();
        foreign_source["supports"][0]["members"][0]["head"]["parts"][0]["source"] = 2.into();
        assert!(serde_json::from_value::<QueryExplanation>(foreign_source).is_err());

        let mut false_body = original.clone();
        let provider_body = false_body["supports"][0]["readingBody"].clone();
        false_body["evidence"][0]["content"]["readingBody"] = provider_body;
        assert!(serde_json::from_value::<QueryExplanation>(false_body).is_err());

        let mut missing_provider = original;
        missing_provider["supports"][0]["readingBody"] = json!({"parts": [], "joins": []});
        assert!(serde_json::from_value::<QueryExplanation>(missing_provider).is_err());
    }

    #[test]
    fn fixed_group_rejects_forged_join_and_column() {
        let mut original = grouped_response();
        let head = json!({
            "parts": [part(1, 1, "--he"), part(2, 2, "lp")],
            "joins": [{"kind": "direct-contact"}]
        });
        original["supports"][0]["members"][0]["head"] = head.clone();
        original["evidence"][0]["entry"]["fixedForms"] = json!([head]);
        assert!(serde_json::from_value::<QueryExplanation>(original.clone()).is_ok());

        let mut false_join = original.clone();
        false_join["evidence"][0]["entry"]["fixedForms"][0]["joins"][0] =
            json!({"kind": "authored-separator", "text": " "});
        assert!(serde_json::from_value::<QueryExplanation>(false_join).is_err());

        let mut false_column = original;
        false_column["evidence"][0]["entry"]["fixedForms"][0]["parts"][0]["column"] = 1.into();
        assert!(serde_json::from_value::<QueryExplanation>(false_column).is_err());
    }
}
