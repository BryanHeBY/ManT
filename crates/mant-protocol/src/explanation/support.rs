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
        } = self;
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
}

impl super::ExplanationContent {
    /// Resolve this source-qualified reference to its original physical owner.
    #[must_use]
    pub fn referenced_owner<'a>(
        &self,
        pool: &'a [ExplanationSupport],
    ) -> Option<mant_ir::EntryOwner<'a>> {
        let Self::DeclarationMember {
            support,
            item_index,
        } = self
        else {
            return None;
        };
        Some(mant_ir::EntryOwner::Definition(
            pool.get(*support)?.items()?.get(*item_index)?,
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
            Self::Entry { block } | Self::Block { block } => range.resolve(block),
            Self::DeclarationMember {
                support,
                item_index,
            } => {
                self.referenced_owner(pool)?;
                let ExplanationSupport::DeclarationGroup { block, .. } = pool.get(*support)?;
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
                        *index = u32::try_from(*item_index).ok()?;
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
                *index = u32::try_from(*item_index).ok()?;
                mapped.resolve(block)
            }
        }
    }
}

impl super::ExplanationEvidence {
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
            && !self.content_omitted
            && !self.support_omitted
            && content.referenced_owner(pool).is_some_and(|owner| {
                owner
                    .facts()
                    .is_some_and(|facts| facts.id.as_str() == self.outline.node.id())
                    && owner.forms().is_some_and(|forms| {
                        self.entry.as_ref().is_none_or(|entry| {
                            entry.forms.is_empty()
                                || forms.iter().eq(entry.forms.iter().map(Vec::as_slice))
                        })
                    })
            })
    }

    fn valid_references(&self, pool: &[ExplanationSupport]) -> bool {
        use super::{EvidenceBasis, ExplanationContent, ExplanationOccurrence};
        if self.support.is_some_and(|index| pool.get(index).is_none())
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
        && pool.iter().all(|support| support.items().is_some())
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
