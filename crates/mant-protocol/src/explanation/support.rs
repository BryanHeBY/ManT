//! Page-local original context, explicitly distinct from alias evidence.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
}

impl ExplanationSupport {
    /// Validate the local/original ranges and exact member identities before
    /// accepting any reference. Context cannot silently point at another owner.
    #[must_use]
    pub fn items(&self) -> Option<&[mant_ir::DefinitionItem]> {
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
        local.resolve(items)
    }

    /// Resolve this group's original member interval, including a contained
    /// fragment. References never follow chains or cross document pools.
    #[must_use]
    pub fn items_in<'a>(&'a self, pool: &'a [Self]) -> Option<&'a [mant_ir::DefinitionItem]> {
        let (block, group) = self.fragment(pool)?;
        let mant_ir::Block::DefinitionList { items, .. } = block else {
            return None;
        };
        group.resolve(items)
    }

    /// Member trails belonging to this group, not its enclosing context.
    #[must_use]
    pub fn members(&self) -> &[crate::OutlineTrail] {
        match self {
            Self::OwnedEntry { .. } => &[],
            Self::DeclarationGroup { members, .. }
            | Self::ContainedDeclarationGroup { members, .. } => members,
        }
    }

    /// Materialized outer source block, if the complete descriptor is valid.
    /// Frontends render this block once and retain each group's own provenance.
    #[must_use]
    pub fn materialized<'a>(&'a self, pool: &'a [Self]) -> Option<&'a mant_ir::Block> {
        if let Self::OwnedEntry { block } = self {
            return block.entry_owner().map(|_| block);
        }
        self.fragment(pool)?;
        match self {
            Self::OwnedEntry { .. } => None,
            Self::DeclarationGroup { block, .. } => Some(block),
            Self::ContainedDeclarationGroup { support, .. } => match pool.get(*support)? {
                Self::DeclarationGroup { block, .. } | Self::OwnedEntry { block } => Some(block),
                Self::ContainedDeclarationGroup { .. } => None,
            },
        }
    }

    fn fragment<'a>(
        &'a self,
        pool: &'a [Self],
    ) -> Option<(&'a mant_ir::Block, mant_ir::DeclarationGroup)> {
        match self {
            Self::OwnedEntry { .. } => None,
            Self::DeclarationGroup { block, .. } => Some((
                block,
                mant_ir::DeclarationGroup {
                    start_item: 0,
                    end_item: self.items()?.len(),
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
                        parent.items()?;
                        block
                    }
                    Self::OwnedEntry { block } => {
                        block.entry_owner()?;
                        block
                    }
                    Self::ContainedDeclarationGroup { .. } => return None,
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
                let items = group.resolve(items)?;
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
}

impl super::ExplanationContent {
    /// Resolve this source-qualified reference to its original physical owner.
    #[must_use]
    pub fn referenced_owner<'a>(
        &self,
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
            let block = super::locations::block_at(fragment.materialized(pool)?, path)?;
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
            pool.get(*support)?.items_in(pool)?.get(*item_index)?,
        ))
    }

    /// Resolve an owner-local position without copying the shared source body.
    #[must_use]
    pub fn resolve_range<'a>(
        &'a self,
        pool: &'a [ExplanationSupport],
        range: &super::ExplanationContentRange,
    ) -> Option<super::ExplanationTextRoot<'a>> {
        use super::{ExplanationBlockStep as Step, ExplanationContentRange as Range};
        match self {
            Self::SharedEntry {
                support,
                path,
                item_index,
            } => {
                self.referenced_owner(pool)?;
                remap_range(range, path, *item_index)?
                    .resolve(pool.get(*support)?.materialized(pool)?)
            }
            Self::Entry { block } | Self::Block { block } => range.resolve(block),
            Self::DeclarationMember {
                support,
                item_index,
            } => {
                self.referenced_owner(pool)?;
                let (block, group) = pool.get(*support)?.fragment(pool)?;
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
                        return mapped.resolve(block);
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
                mapped.resolve(block)
            }
        }
    }
}

impl super::ExplanationEvidence {
    /// Validated reference to returned original source, regardless of whether
    /// it also carries a declaration-group relationship.
    #[must_use]
    pub fn source_reference(&self, pool: &[ExplanationSupport]) -> Option<usize> {
        if self.covered_by_support(pool) {
            self.support
        } else {
            self.shared_entry(pool)
        }
    }
    /// Validated physical-entry reference, distinct from a group relationship.
    #[must_use]
    pub fn shared_entry(&self, pool: &[ExplanationSupport]) -> Option<usize> {
        let content @ super::ExplanationContent::SharedEntry { support, .. } =
            self.content.as_ref()?
        else {
            return None;
        };
        (self.class == super::EvidenceClass::DirectEntry
            && self.support.is_none()
            && !self.content_omitted
            && content
                .referenced_owner(pool)
                .is_some_and(|owner| self.matches_owner(owner)))
        .then_some(*support)
    }
    /// Whether a shared source fragment covers this exact owner and its forms.
    /// Invalid references must never hide separately returned metadata.
    #[must_use]
    pub fn covered_by_support(&self, pool: &[ExplanationSupport]) -> bool {
        let Some(content @ super::ExplanationContent::DeclarationMember { support, .. }) =
            &self.content
        else {
            return false;
        };
        self.support == Some(*support)
            && self.class == super::EvidenceClass::DirectEntry
            && !self.content_omitted
            && !self.support_omitted
            && content
                .referenced_owner(pool)
                .is_some_and(|owner| self.matches_owner(owner))
    }

    fn matches_owner(&self, owner: mant_ir::EntryOwner<'_>) -> bool {
        owner
            .facts()
            .is_some_and(|facts| facts.id.as_str() == self.outline.node.id())
            && owner.forms().is_some_and(|forms| {
                self.entry.as_ref().is_none_or(|entry| {
                    entry.forms.is_empty() || forms.iter().eq(entry.forms.iter().map(Vec::as_slice))
                })
            })
    }

    fn valid_references(&self, pool: &[ExplanationSupport]) -> bool {
        use super::{EvidenceBasis, ExplanationContent, ExplanationOccurrence};
        if matches!(self.content, Some(ExplanationContent::SharedEntry { .. }))
            && self.shared_entry(pool).is_none()
            || self.support_omitted && self.class != super::EvidenceClass::DirectEntry
            || self.content_omitted && self.content.is_some()
            || self.support.is_some() && !self.covered_by_support(pool)
            || self.support.is_some_and(|index| pool.get(index).is_none())
            || self.support.is_some() && self.support_omitted
            || matches!(
                self.content,
                Some(ExplanationContent::DeclarationMember { .. })
            ) && !self.covered_by_support(pool)
        {
            return false;
        }
        let valid_content = |range: &super::ExplanationContentRange| {
            self.content
                .as_ref()
                .is_some_and(|content| content.resolve_range(pool, range).is_some())
        };
        let valid_occurrence = |occurrence: &ExplanationOccurrence| {
            occurrence.forms.iter().all(|range| {
                self.entry
                    .as_ref()
                    .is_some_and(|entry| range.resolve(&entry.forms).is_some())
            }) && occurrence.content.iter().all(&valid_content)
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

fn valid_pool(pool: &[ExplanationSupport]) -> bool {
    pool.len() <= crate::MAX_EXPLANATION_RESULTS as usize
        && pool
            .iter()
            .all(|support| support.materialized(pool).is_some())
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
        (valid_pool(&self.supports)
            && self
                .evidence
                .iter()
                .all(|e| e.valid_references(&self.supports)))
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
        (self.documents.iter().all(|d| valid_pool(&d.supports))
            && self.evidence.iter().all(|e| {
                self.documents
                    .get(e.document_index)
                    .is_some_and(|d| e.evidence.valid_references(&d.supports))
            }))
        .then_some(())
        .ok_or("invalid scoped explanation source reference or position")
    }
}
