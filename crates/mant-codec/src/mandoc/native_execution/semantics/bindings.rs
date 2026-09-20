//! Stable semantic bindings for the K23 native projection route.
//!
//! Native semantic evidence is bound while a definition item is materialized.
//! The binding keeps the complete final owner path, including its terminal
//! [`ContentBlockStep::DefinitionItem`].  Consequently evidence ordering and
//! receipt serialization never walk rendered IR, compare source coordinates,
//! or insert temporary marker anchors.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
};

use libmandoc_rs::ExecutionNodeKey;
use mant_ir::{Block, ContentBlockStep, Section};

use super::super::ownership::{ProjectionDestination, ProjectionNodeKey, SectionKey};

/// Stable identity of one native declaration owner in the sealed arena.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct StableSemanticOwner {
    pub(super) projection: ProjectionNodeKey,
    pub(super) native: ExecutionNodeKey,
}

/// Stable identity of one materialized definition item.
///
/// A man(7) `.TP` owner and all immediately continued `.TQ` owners bind to
/// the same item, while retaining distinct [`StableSemanticOwner`] values.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct StableSemanticItem(pub(super) ProjectionNodeKey);

/// Complete final structural address of a semantic item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SemanticItemAddress {
    pub(super) destination: ProjectionDestination,
    /// Complete path ending in the owned `DefinitionItem`.
    pub(super) owner_path: Vec<ContentBlockStep>,
}

impl Ord for SemanticItemAddress {
    fn cmp(&self, other: &Self) -> Ordering {
        destination_order(self.destination)
            .cmp(&destination_order(other.destination))
            .then_with(|| self.owner_path.cmp(&other.owner_path))
    }
}

impl PartialOrd for SemanticItemAddress {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

const fn destination_order(destination: ProjectionDestination) -> (u8, u32) {
    match destination {
        ProjectionDestination::Root => (0, 0),
        ProjectionDestination::Section(section) => (1, section.0),
        ProjectionDestination::Deferred => (2, 0),
    }
}

impl SemanticItemAddress {
    pub(super) fn new(
        destination: ProjectionDestination,
        owner_path: Vec<ContentBlockStep>,
    ) -> Result<Self, SemanticBindingError> {
        let alternating = owner_path.len() >= 2
            && owner_path.len() % 2 == 0
            && owner_path.iter().enumerate().all(|(index, step)| {
                if index % 2 == 0 {
                    matches!(step, ContentBlockStep::Block { .. })
                } else {
                    !matches!(step, ContentBlockStep::Block { .. })
                }
            });
        if !alternating
            || !matches!(
                owner_path.last(),
                Some(ContentBlockStep::DefinitionItem { .. })
            )
        {
            return Err(SemanticBindingError::InvalidOwnerPath);
        }
        Ok(Self {
            destination,
            owner_path,
        })
    }

    fn receipt_address(
        &self,
        section_paths: &BTreeMap<SectionKey, Vec<u32>>,
    ) -> Result<SemanticReceiptAddress, SemanticBindingError> {
        let mut blocks = self.owner_path.clone();
        let Some(ContentBlockStep::DefinitionItem { index }) = blocks.pop() else {
            return Err(SemanticBindingError::InvalidOwnerPath);
        };
        let sections = match self.destination {
            ProjectionDestination::Root => Vec::new(),
            ProjectionDestination::Section(section) => section_paths
                .get(&section)
                .cloned()
                .ok_or(SemanticBindingError::MissingSectionAddress(section))?,
            ProjectionDestination::Deferred => {
                return Err(SemanticBindingError::DeferredDestination);
            }
        };
        Ok(SemanticReceiptAddress {
            sections,
            blocks,
            item_index: index,
        })
    }
}

/// Wire-ready address fields for [`super::NativeSemanticReceipt`].
///
/// `blocks` and `item_index` are split directly from the registered complete
/// owner path; no final-document lookup is involved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SemanticReceiptAddress {
    pub(super) sections: Vec<u32>,
    pub(super) blocks: Vec<ContentBlockStep>,
    pub(super) item_index: u32,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SemanticDfsAddress {
    destination_kind: u8,
    sections: Vec<u32>,
    owner_path: Vec<ContentBlockStep>,
}

impl SemanticDfsAddress {
    fn new(address: &SemanticItemAddress, receipt: &SemanticReceiptAddress) -> Self {
        Self {
            destination_kind: u8::from(!matches!(address.destination, ProjectionDestination::Root)),
            sections: receipt.sections.clone(),
            owner_path: address.owner_path.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct OwnerBinding {
    item: StableSemanticItem,
    term_ordinal: u32,
}

/// One semantic item in exact final DFS order.
#[derive(Clone, Debug)]
pub(super) struct BoundSemanticItem {
    pub(super) receipt: SemanticReceiptAddress,
    pub(super) origins: Vec<ExecutionNodeKey>,
    pub(super) evidence: crate::definitions::ExactNativeDefinitionEvidence,
}

/// Sealed K23 semantic bindings, ready for exact identity allocation.
#[derive(Clone, Debug, Default)]
pub(super) struct SemanticBindingPlan {
    pub(super) items: Vec<BoundSemanticItem>,
}

impl SemanticBindingPlan {
    pub(super) fn evidence_stream(&self) -> Vec<crate::definitions::ExactNativeDefinitionEvidence> {
        self.items
            .iter()
            .map(|item| item.evidence.clone())
            .collect()
    }

    /// Apply exact definition identities without an infallible legacy bridge.
    pub(super) fn identify(
        &self,
        blocks: &mut [Block],
        sections: &mut [Section],
        reserved_targets: &HashSet<String>,
        document_name: Option<&str>,
        target_aliases: &HashMap<String, String>,
        authored_titles: &HashMap<String, String>,
    ) -> Result<
        crate::definitions::ExactNativeIdentityResult,
        crate::definitions::ExactNativeIdentityError,
    > {
        crate::definitions::identify_exact_native_definitions_checked(
            blocks,
            sections,
            reserved_targets,
            document_name,
            self.evidence_stream(),
            target_aliases,
            authored_titles,
        )
    }

    /// Serialize receipt coordinates by zipping the exact allocator result
    /// with this already DFS-ordered plan.  No public IR walk is performed.
    pub(super) fn receipts(
        &self,
        identities: &crate::definitions::ExactNativeIdentityResult,
    ) -> Result<Vec<super::NativeSemanticReceipt>, SemanticBindingError> {
        if identities.allocated.len() != self.items.len() {
            return Err(SemanticBindingError::IdentityCardinalityMismatch {
                items: self.items.len(),
                identities: identities.allocated.len(),
            });
        }
        Ok(self
            .items
            .iter()
            .zip(&identities.allocated)
            .filter_map(|(item, id)| {
                Some(super::NativeSemanticReceipt {
                    origins: item.origins.clone(),
                    id: id.clone()?,
                    sections: item.receipt.sections.clone(),
                    blocks: item.receipt.blocks.clone(),
                    item_index: item.receipt.item_index,
                })
            })
            .collect())
    }
}

/// Builder for the stable owner → item → final-address relation.
#[derive(Clone, Debug, Default)]
pub(super) struct SemanticBindingRegistry {
    items: BTreeMap<StableSemanticItem, SemanticItemAddress>,
    address_owners: BTreeMap<SemanticItemAddress, StableSemanticItem>,
    owners: BTreeMap<StableSemanticOwner, OwnerBinding>,
    evidence: BTreeMap<StableSemanticOwner, crate::definitions::ExactNativeDefinitionEvidence>,
    section_paths: BTreeMap<SectionKey, Vec<u32>>,
    section_path_owners: BTreeMap<Vec<u32>, SectionKey>,
}

impl SemanticBindingRegistry {
    pub(super) fn register_section_address(
        &mut self,
        section: SectionKey,
        sections: Vec<u32>,
    ) -> Result<(), SemanticBindingError> {
        if sections.is_empty() {
            return Err(SemanticBindingError::InvalidSectionAddress(section));
        }
        if let Some(first) = self.section_path_owners.get(&sections)
            && *first != section
        {
            return Err(SemanticBindingError::DuplicateSectionPath {
                first: *first,
                second: section,
            });
        }
        match self.section_paths.entry(section) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                self.section_path_owners.insert(sections.clone(), section);
                entry.insert(sections);
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &sections => {
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                Err(SemanticBindingError::DuplicateSectionAddress(section))
            }
        }
    }

    pub(super) fn register_item(
        &mut self,
        item: StableSemanticItem,
        address: SemanticItemAddress,
    ) -> Result<(), SemanticBindingError> {
        if let Some(existing) = self.items.get(&item) {
            return if existing == &address {
                Ok(())
            } else {
                Err(SemanticBindingError::DuplicateItem(item))
            };
        }
        if let Some(existing) = self.address_owners.get(&address) {
            return Err(SemanticBindingError::DuplicateItemAddress {
                first: *existing,
                second: item,
            });
        }
        self.address_owners.insert(address.clone(), item);
        self.items.insert(item, address);
        Ok(())
    }

    /// Bind one native declaration to one exact term in a registered item.
    pub(super) fn bind_owner(
        &mut self,
        owner: StableSemanticOwner,
        item: StableSemanticItem,
        term_ordinal: u32,
    ) -> Result<(), SemanticBindingError> {
        if !self.items.contains_key(&item) {
            return Err(SemanticBindingError::UnknownItem(item));
        }
        match self.owners.entry(owner) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(OwnerBinding { item, term_ordinal });
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(entry)
                if entry.get().item == item && entry.get().term_ordinal == term_ordinal =>
            {
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                Err(SemanticBindingError::DuplicateOwner(owner))
            }
        }
    }

    pub(super) fn register_evidence(
        &mut self,
        owner: StableSemanticOwner,
        evidence: crate::definitions::ExactNativeDefinitionEvidence,
    ) -> Result<(), SemanticBindingError> {
        if self.evidence.insert(owner, evidence).is_some() {
            return Err(SemanticBindingError::DuplicateEvidence(owner));
        }
        Ok(())
    }

    /// Validate the complete relation and emit items in final document DFS.
    pub(super) fn seal(mut self) -> Result<SemanticBindingPlan, SemanticBindingError> {
        if let Some(owner) = self
            .evidence
            .keys()
            .find(|owner| !self.owners.contains_key(owner))
            .copied()
        {
            return Err(SemanticBindingError::UnaddressedEvidence(owner));
        }

        let mut owners_by_item =
            BTreeMap::<StableSemanticItem, BTreeMap<u32, StableSemanticOwner>>::new();
        for (owner, binding) in &self.owners {
            if !self.evidence.contains_key(owner) {
                return Err(SemanticBindingError::MissingEvidence(*owner));
            }
            let terms = owners_by_item.entry(binding.item).or_default();
            if let Some(first) = terms.insert(binding.term_ordinal, *owner) {
                return Err(SemanticBindingError::DuplicateTermOrdinal {
                    item: binding.item,
                    ordinal: binding.term_ordinal,
                    first,
                    second: *owner,
                });
            }
        }

        let mut ordered = BTreeMap::<
            SemanticDfsAddress,
            (
                StableSemanticItem,
                SemanticItemAddress,
                SemanticReceiptAddress,
            ),
        >::new();
        for (item, address) in &self.items {
            if !owners_by_item.contains_key(item) {
                return Err(SemanticBindingError::ItemWithoutOwners(*item));
            }
            let receipt = address.receipt_address(&self.section_paths)?;
            let dfs = SemanticDfsAddress::new(address, &receipt);
            if let Some((first, _, _)) = ordered.insert(dfs, (*item, address.clone(), receipt)) {
                return Err(SemanticBindingError::DuplicateReceiptAddress {
                    first,
                    second: *item,
                });
            }
        }

        let mut items = Vec::with_capacity(ordered.len());
        for (_, (item, _address, receipt)) in ordered {
            let terms = owners_by_item
                .remove(&item)
                .ok_or(SemanticBindingError::ItemWithoutOwners(item))?;
            let actual = terms.keys().copied().collect::<BTreeSet<_>>();
            let expected = (0..u32::try_from(terms.len())
                .map_err(|_| SemanticBindingError::TermOrdinalOverflow(item))?)
                .collect::<BTreeSet<_>>();
            if actual != expected {
                return Err(SemanticBindingError::NonContiguousTerms {
                    item,
                    ordinals: actual.into_iter().collect(),
                });
            }

            let mut origins = Vec::with_capacity(terms.len());
            let mut combined = crate::definitions::ExactNativeDefinitionEvidence::default();
            for (term_ordinal, owner) in terms {
                let mut next = self
                    .evidence
                    .remove(&owner)
                    .ok_or(SemanticBindingError::MissingEvidence(owner))?;
                next.shift_terms(
                    usize::try_from(term_ordinal)
                        .map_err(|_| SemanticBindingError::TermOrdinalOverflow(item))?,
                );
                combined.append(next);
                origins.push(owner.native);
            }
            items.push(BoundSemanticItem {
                receipt,
                origins,
                evidence: combined,
            });
        }
        debug_assert!(self.evidence.is_empty());
        Ok(SemanticBindingPlan { items })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SemanticBindingError {
    DeferredDestination,
    InvalidOwnerPath,
    DuplicateSectionAddress(SectionKey),
    DuplicateSectionPath {
        first: SectionKey,
        second: SectionKey,
    },
    InvalidSectionAddress(SectionKey),
    MissingSectionAddress(SectionKey),
    DuplicateItem(StableSemanticItem),
    DuplicateItemAddress {
        first: StableSemanticItem,
        second: StableSemanticItem,
    },
    DuplicateReceiptAddress {
        first: StableSemanticItem,
        second: StableSemanticItem,
    },
    UnknownItem(StableSemanticItem),
    DuplicateOwner(StableSemanticOwner),
    DuplicateEvidence(StableSemanticOwner),
    MissingEvidence(StableSemanticOwner),
    UnaddressedEvidence(StableSemanticOwner),
    ItemWithoutOwners(StableSemanticItem),
    DuplicateTermOrdinal {
        item: StableSemanticItem,
        ordinal: u32,
        first: StableSemanticOwner,
        second: StableSemanticOwner,
    },
    NonContiguousTerms {
        item: StableSemanticItem,
        ordinals: Vec<u32>,
    },
    TermOrdinalOverflow(StableSemanticItem),
    IdentityCardinalityMismatch {
        items: usize,
        identities: usize,
    },
}

impl std::fmt::Display for SemanticBindingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeferredDestination => {
                formatter.write_str("semantic owner destination is deferred")
            }
            Self::InvalidOwnerPath => {
                formatter.write_str("semantic owner path is not a complete definition-item address")
            }
            Self::DuplicateSectionAddress(section) => write!(
                formatter,
                "section {section:?} has conflicting final addresses"
            ),
            Self::DuplicateSectionPath { first, second } => write!(
                formatter,
                "sections {first:?} and {second:?} share a final address"
            ),
            Self::InvalidSectionAddress(section) => {
                write!(formatter, "section {section:?} has an empty final address")
            }
            Self::MissingSectionAddress(section) => {
                write!(formatter, "section {section:?} has no final address")
            }
            Self::DuplicateItem(item) => write!(
                formatter,
                "semantic item {item:?} has conflicting addresses"
            ),
            Self::DuplicateItemAddress { first, second } => write!(
                formatter,
                "semantic items {first:?} and {second:?} share an arena address"
            ),
            Self::DuplicateReceiptAddress { first, second } => write!(
                formatter,
                "semantic items {first:?} and {second:?} share a final receipt address"
            ),
            Self::UnknownItem(item) => {
                write!(formatter, "semantic owner refers to unknown item {item:?}")
            }
            Self::DuplicateOwner(owner) => write!(
                formatter,
                "semantic owner {owner:?} has conflicting bindings"
            ),
            Self::DuplicateEvidence(owner) => {
                write!(formatter, "semantic owner {owner:?} has duplicate evidence")
            }
            Self::MissingEvidence(owner) => {
                write!(formatter, "semantic owner {owner:?} has no evidence")
            }
            Self::UnaddressedEvidence(owner) => write!(
                formatter,
                "semantic evidence {owner:?} has no materialized owner"
            ),
            Self::ItemWithoutOwners(item) => {
                write!(formatter, "semantic item {item:?} has no native owners")
            }
            Self::DuplicateTermOrdinal { item, ordinal, .. } => write!(
                formatter,
                "semantic item {item:?} binds term ordinal {ordinal} more than once"
            ),
            Self::NonContiguousTerms { item, ordinals } => write!(
                formatter,
                "semantic item {item:?} has non-contiguous term ordinals {ordinals:?}"
            ),
            Self::TermOrdinalOverflow(item) => write!(
                formatter,
                "semantic item {item:?} term ordinal exceeds address capacity"
            ),
            Self::IdentityCardinalityMismatch { items, identities } => write!(
                formatter,
                "semantic binding plan has {items} items but exact identity allocation returned {identities} results"
            ),
        }
    }
}

impl std::error::Error for SemanticBindingError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definitions::{
        ExactNativeDefinitionEvidence, ExactNativeNameEvidence, NativeHeadRole,
    };

    fn owner(value: u32) -> StableSemanticOwner {
        StableSemanticOwner {
            projection: ProjectionNodeKey(value),
            native: ExecutionNodeKey(value),
        }
    }

    fn item(value: u32) -> StableSemanticItem {
        StableSemanticItem(ProjectionNodeKey(value))
    }

    fn root_address(path: Vec<ContentBlockStep>) -> SemanticItemAddress {
        SemanticItemAddress::new(ProjectionDestination::Root, path).unwrap()
    }

    fn evidence(role: NativeHeadRole) -> ExactNativeDefinitionEvidence {
        ExactNativeDefinitionEvidence {
            role: Some(role),
            markup: vec![ExactNativeNameEvidence {
                term_index: 0,
                parts: Vec::new(),
            }],
            ..ExactNativeDefinitionEvidence::default()
        }
    }

    fn register(
        registry: &mut SemanticBindingRegistry,
        item: StableSemanticItem,
        owner: StableSemanticOwner,
        address: SemanticItemAddress,
        evidence: ExactNativeDefinitionEvidence,
    ) {
        registry.register_item(item, address).unwrap();
        registry.bind_owner(owner, item, 0).unwrap();
        registry.register_evidence(owner, evidence).unwrap();
    }

    #[test]
    fn final_dfs_order_is_outer_zero_then_nested_then_outer_one() {
        let mut registry = SemanticBindingRegistry::default();
        register(
            &mut registry,
            item(10),
            owner(1),
            root_address(vec![
                ContentBlockStep::Block { index: 0 },
                ContentBlockStep::DefinitionItem { index: 0 },
            ]),
            evidence(NativeHeadRole::Option),
        );
        register(
            &mut registry,
            item(11),
            owner(2),
            root_address(vec![
                ContentBlockStep::Block { index: 0 },
                ContentBlockStep::DefinitionItem { index: 0 },
                ContentBlockStep::Block { index: 0 },
                ContentBlockStep::DefinitionItem { index: 0 },
            ]),
            evidence(NativeHeadRole::Environment),
        );
        register(
            &mut registry,
            item(12),
            owner(3),
            root_address(vec![
                ContentBlockStep::Block { index: 0 },
                ContentBlockStep::DefinitionItem { index: 1 },
            ]),
            evidence(NativeHeadRole::Literal),
        );

        let plan = registry.seal().unwrap();
        assert_eq!(
            plan.evidence_stream()
                .into_iter()
                .map(|entry| entry.role.unwrap())
                .collect::<Vec<_>>(),
            [
                NativeHeadRole::Option,
                NativeHeadRole::Environment,
                NativeHeadRole::Literal,
            ]
        );
        assert_eq!(plan.items[1].receipt.item_index, 0);
        assert_eq!(
            plan.items[1].receipt.blocks,
            [
                ContentBlockStep::Block { index: 0 },
                ContentBlockStep::DefinitionItem { index: 0 },
                ContentBlockStep::Block { index: 0 },
            ]
        );
    }

    #[test]
    fn tq_owners_merge_into_one_item_by_term_ordinal() {
        let mut registry = SemanticBindingRegistry::default();
        let item = item(10);
        registry
            .register_item(
                item,
                root_address(vec![
                    ContentBlockStep::Block { index: 0 },
                    ContentBlockStep::DefinitionItem { index: 0 },
                ]),
            )
            .unwrap();
        for (ordinal, owner) in [(0, owner(1)), (1, owner(2)), (2, owner(3))] {
            registry.bind_owner(owner, item, ordinal).unwrap();
            registry
                .register_evidence(owner, evidence(NativeHeadRole::Option))
                .unwrap();
        }

        let plan = registry.seal().unwrap();
        assert_eq!(plan.items.len(), 1);
        assert_eq!(
            plan.items[0]
                .evidence
                .markup
                .iter()
                .map(|name| name.term_index)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(
            plan.items[0].origins,
            [
                ExecutionNodeKey(1),
                ExecutionNodeKey(2),
                ExecutionNodeKey(3)
            ]
        );
        let receipts = plan
            .receipts(&crate::definitions::ExactNativeIdentityResult {
                retained: HashSet::new(),
                groupable: HashSet::new(),
                allocated: vec![Some("option-probe".into())],
            })
            .unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].id.as_str(), "option-probe");
        assert_eq!(receipts[0].item_index, 0);
        assert_eq!(receipts[0].origins, plan.items[0].origins);
    }

    #[test]
    fn duplicate_term_ordinal_is_rejected() {
        let mut registry = SemanticBindingRegistry::default();
        let item = item(10);
        registry
            .register_item(
                item,
                root_address(vec![
                    ContentBlockStep::Block { index: 0 },
                    ContentBlockStep::DefinitionItem { index: 0 },
                ]),
            )
            .unwrap();
        registry.bind_owner(owner(1), item, 0).unwrap();
        registry.bind_owner(owner(2), item, 0).unwrap();
        registry
            .register_evidence(owner(1), evidence(NativeHeadRole::Option))
            .unwrap();
        registry
            .register_evidence(owner(2), evidence(NativeHeadRole::Option))
            .unwrap();

        assert!(matches!(
            registry.seal(),
            Err(SemanticBindingError::DuplicateTermOrdinal { ordinal: 0, .. })
        ));
    }

    #[test]
    fn unaddressed_evidence_is_rejected() {
        let mut registry = SemanticBindingRegistry::default();
        registry
            .register_evidence(owner(1), evidence(NativeHeadRole::Option))
            .unwrap();
        assert_eq!(
            registry.seal().unwrap_err(),
            SemanticBindingError::UnaddressedEvidence(owner(1))
        );
    }

    #[test]
    fn addressed_owner_without_evidence_is_rejected() {
        let mut registry = SemanticBindingRegistry::default();
        let item = item(10);
        registry
            .register_item(
                item,
                root_address(vec![
                    ContentBlockStep::Block { index: 0 },
                    ContentBlockStep::DefinitionItem { index: 0 },
                ]),
            )
            .unwrap();
        registry.bind_owner(owner(1), item, 0).unwrap();
        assert_eq!(
            registry.seal().unwrap_err(),
            SemanticBindingError::MissingEvidence(owner(1))
        );
    }

    #[test]
    fn section_dfs_uses_serialized_section_path_not_arena_key() {
        let mut registry = SemanticBindingRegistry::default();
        registry
            .register_section_address(SectionKey(9), vec![0])
            .unwrap();
        registry
            .register_section_address(SectionKey(1), vec![1])
            .unwrap();
        register(
            &mut registry,
            item(10),
            owner(1),
            SemanticItemAddress::new(
                ProjectionDestination::Section(SectionKey(9)),
                vec![
                    ContentBlockStep::Block { index: 0 },
                    ContentBlockStep::DefinitionItem { index: 0 },
                ],
            )
            .unwrap(),
            evidence(NativeHeadRole::Option),
        );
        register(
            &mut registry,
            item(11),
            owner(2),
            SemanticItemAddress::new(
                ProjectionDestination::Section(SectionKey(1)),
                vec![
                    ContentBlockStep::Block { index: 0 },
                    ContentBlockStep::DefinitionItem { index: 0 },
                ],
            )
            .unwrap(),
            evidence(NativeHeadRole::Literal),
        );

        let plan = registry.seal().unwrap();
        assert_eq!(plan.items[0].receipt.sections, [0]);
        assert_eq!(plan.items[1].receipt.sections, [1]);
        assert_eq!(
            plan.evidence_stream()
                .into_iter()
                .map(|entry| entry.role.unwrap())
                .collect::<Vec<_>>(),
            [NativeHeadRole::Option, NativeHeadRole::Literal]
        );
    }

    #[test]
    fn non_contiguous_terms_are_rejected() {
        let mut registry = SemanticBindingRegistry::default();
        let item = item(10);
        registry
            .register_item(
                item,
                root_address(vec![
                    ContentBlockStep::Block { index: 0 },
                    ContentBlockStep::DefinitionItem { index: 0 },
                ]),
            )
            .unwrap();
        for (ordinal, owner) in [(0, owner(1)), (2, owner(2))] {
            registry.bind_owner(owner, item, ordinal).unwrap();
            registry
                .register_evidence(owner, evidence(NativeHeadRole::Option))
                .unwrap();
        }
        assert_eq!(
            registry.seal().unwrap_err(),
            SemanticBindingError::NonContiguousTerms {
                item,
                ordinals: vec![0, 2],
            }
        );
    }
}
