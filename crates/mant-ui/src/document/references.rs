//! Bounded sidebar inventory of real content references, never semantic entries.
use std::{
    collections::{BTreeMap, HashMap},
    ops::ControlFlow,
    sync::Arc,
};

use mant_ir::{ContentLocation, ContentLocationRef, Document, LinkTarget, ReferenceScanLimits};

use super::{NavKind, NavNode, ROOT_ID};

const MAX_RECORDS: usize = 1000;
const MAX_PAYLOAD: usize = 1024 * 1024;
const MAX_LABEL: usize = 4096;

/// Operation-only identity map; pointer keys never leave this build or its IR.
pub(super) type ReferenceOrigins = HashMap<usize, Arc<str>>;

#[derive(Debug, Clone)]
pub(super) struct ReferenceRecord {
    pub(super) id: Arc<str>,
    pub(super) location: ContentLocation,
    pub(super) owner: String,
    fallback_owner: String,
    pub(super) label: String,
    pub(super) target: LinkTarget,
    attachment: mant_protocol::ReferenceAttachment,
}

#[derive(Debug, Default)]
pub(super) struct ReferenceNavigation {
    pub(super) records: Vec<ReferenceRecord>,
    pub(super) origins: ReferenceOrigins,
    pub(super) limited: bool,
    pub(super) associated: HashMap<String, Vec<usize>>,
    source_owner_counts: HashMap<String, u8>,
    source_owners_verified: bool,
    remaining_budget: Option<mant_ir::ReferenceWorkBudget>,
}

impl ReferenceNavigation {
    pub(super) fn build(document: &Document) -> Self {
        let mut result = Self::default();
        let mut payload = 0usize;
        let mut budget = mant_ir::ReferenceWorkBudget::new(ReferenceScanLimits::default());
        let report = mant_ir::scan_navigation_scope_with_budget(
            document,
            mant_ir::ReferenceScope::Document,
            &mut budget,
            mant_ir::NavigationScanOptions {
                links: mant_ir::ReferenceLinkFilter::DOCUMENTS,
                targets: false,
                entry_sets: false,
            },
            |event, budget| {
                let mant_ir::NavigationEvent::Link(occurrence) = event else {
                    return ControlFlow::Continue(());
                };
                if result.records.len() == MAX_RECORDS {
                    result.limited = true;
                    return ControlFlow::Break(());
                }
                // Repeated position inspection, source-owner lookup, copying and
                // private-key serialization are part of this same operation.
                let depth = occurrence.location.depth();
                if budget
                    .consume(depth, depth.saturating_mul(8).saturating_add(1), 0)
                    .is_err()
                {
                    result.limited = true;
                    return ControlFlow::Break(());
                }
                let fallback_owner = match occurrence.location {
                    ContentLocationRef::DocumentHeading { .. } => ROOT_ID,
                    ContentLocationRef::SectionHeading { sections, .. }
                    | ContentLocationRef::Content { sections, .. } => {
                        mant_ir::resolve_content_section(document, sections)
                            .map_or(ROOT_ID, |section| section.id.as_str())
                    }
                };
                let semantic_owner = occurrence
                    .semantic_owner
                    .and_then(|owner| owner.owner.facts())
                    .filter(|facts| mant_ir::is_normalized_node_id(facts.id.as_str()));
                let owner = semantic_owner.map_or(fallback_owner, |facts| facts.id.as_str());
                let target_len = target_bytes(occurrence.target);
                let base = occurrence
                    .location
                    .encoded_len()
                    .saturating_mul(4)
                    .saturating_add(target_len.saturating_mul(4))
                    .saturating_add(owner.len().saturating_mul(4))
                    .saturating_add(fallback_owner.len())
                    .saturating_add(512);
                if base > MAX_PAYLOAD.saturating_sub(payload) {
                    result.limited = true;
                    return ControlFlow::Break(());
                }
                let Some(location) = occurrence.location.to_owned() else {
                    result.limited = true;
                    return ControlFlow::Break(());
                };
                let label_limit = MAX_LABEL.min(
                    MAX_PAYLOAD
                        .saturating_sub(payload)
                        .saturating_sub(base)
                        .saturating_sub(64)
                        / 9,
                );
                let Some(label) = display_label(occurrence, budget, label_limit) else {
                    result.limited = true;
                    return ControlFlow::Break(());
                };
                let cost = base.saturating_add(label.len().saturating_mul(3));
                if cost > MAX_PAYLOAD.saturating_sub(payload) {
                    result.limited = true;
                    return ControlFlow::Break(());
                }
                payload += cost;
                // The key contains only typed structural coordinates, never label,
                // target, page ordinal or a rendered row. Debug spelling is private
                // to this immutable view; it is not a public selector or wire ID.
                let id: Arc<str> = format!("reference:{location:?}").into();
                let (attachment, limited) =
                    reference_attachment(occurrence, budget, &location, semantic_owner.is_some());
                result.limited |= limited;
                result.origins.insert(
                    std::ptr::from_ref(occurrence.target).addr(),
                    Arc::clone(&id),
                );
                result.records.push(ReferenceRecord {
                    id,
                    location,
                    owner: owner.to_owned(),
                    fallback_owner: fallback_owner.to_owned(),
                    label,
                    target: occurrence.target.clone(),
                    attachment,
                });
                ControlFlow::Continue(())
            },
        );
        result.limited |= !report.complete();
        result.remaining_budget = Some(budget);
        result
    }

    pub(super) fn check_source_owners(
        &mut self,
        document: &Document,
        index: &mant_ir::SemanticIndex,
    ) {
        // A hidden owner must not attach to an unrelated visible node with the
        // same ID. Reuse the original semantic index and section metadata,
        // not filtered navigation or another traversal of the entire body.
        // The remaining shared budget bounds this small ownership census.
        self.source_owner_counts = self
            .records
            .iter()
            .flat_map(|record| [&record.owner, &record.fallback_owner])
            .map(|id| (id.clone(), 0))
            .collect();
        if self.source_owner_counts.is_empty() {
            return;
        }
        let Some(mut budget) = self.remaining_budget.take() else {
            return;
        };
        let mut census = OwnerCensus {
            counts: &mut self.source_owner_counts,
            budget: &mut budget,
        };
        let checked = (|| {
            if document.heading.is_some()
                || !document.blocks.is_empty()
                || !document.fragment_aliases.is_empty()
            {
                census.count(ROOT_ID, 0)?;
            }
            census.entries(index.root(), 0)?;
            census.sections(&document.sections, index, &mut Vec::new())
        })();
        self.source_owners_verified = checked.is_ok();
        self.limited |= checked.is_err();
    }

    pub(super) fn badges(&self) -> HashMap<String, String> {
        self.associated
            .iter()
            .map(|(owner, indices)| {
                let badge = mant_protocol::reference_badge(
                    indices.iter().map(|index| &self.records[*index].target),
                    !self.limited,
                );
                (owner.clone(), badge)
            })
            .collect()
    }

    /// Insert orthogonal reference groups after the owner's content children.
    pub(super) fn append_navigation(&mut self, nodes: &mut Vec<NavNode>) {
        if self.records.is_empty() && !self.limited {
            return;
        }
        // Index only requested owners (at most twice the bounded record count),
        // not all document IDs; avoid a full sidebar scan per occurrence.
        let mut owner_counts: HashMap<&str, u8> = self
            .records
            .iter()
            .flat_map(|record| [record.owner.as_str(), record.fallback_owner.as_str()])
            .map(|owner| (owner, 0))
            .collect();
        for node in nodes.iter() {
            if let Some(count) = owner_counts.get_mut(node.id.as_str()) {
                *count = count.saturating_add(1).min(2);
            }
        }
        let mut by_owner: BTreeMap<&str, Vec<&ReferenceRecord>> = BTreeMap::new();
        let mut orphaned = Vec::new();
        for (index, record) in self.records.iter().enumerate() {
            let owner = if self.source_owners_verified
                && self.source_owner_counts.get(record.owner.as_str()) == Some(&1)
                && owner_counts.get(record.owner.as_str()) == Some(&1)
            {
                record.owner.as_str()
            } else if self.source_owners_verified
                && self.source_owner_counts.get(record.fallback_owner.as_str()) == Some(&1)
                && owner_counts.get(record.fallback_owner.as_str()) == Some(&1)
            {
                record.fallback_owner.as_str()
            } else {
                // Invalid duplicate IDs must not attach every occurrence to
                // several unrelated content owners. Keep each source once.
                orphaned.push(record);
                continue;
            };
            if owner == record.owner
                && record.attachment != mant_protocol::ReferenceAttachment::Body
            {
                self.associated
                    .entry(owner.to_owned())
                    .or_default()
                    .push(index);
            } else {
                by_owner.entry(owner).or_default().push(record);
            }
        }
        let mut original = std::mem::take(nodes).into_iter().peekable();
        while original.peek().is_some() {
            append_owner_navigation(&mut original, nodes, &by_owner);
        }
        if !orphaned.is_empty() {
            let group_id = "references:unresolved-owner";
            nodes.push(NavNode {
                id: group_id.into(),
                target_id: orphaned[0].id.to_string(),
                title: "DOCUMENT REFERENCES · unresolved owner".into(),
                full_title: None,
                depth: 0,
                kind: NavKind::ReferenceGroup,
                has_children: true,
                is_last: true,
                parent_id: None,
            });
            for (index, record) in orphaned.iter().enumerate() {
                nodes.push(reference_node(
                    record,
                    1,
                    index + 1 == orphaned.len(),
                    group_id,
                ));
            }
        }
        if self.limited {
            nodes.push(NavNode {
                id: "references-limited".into(),
                target_id: String::new(),
                title: "References limited by navigation budget".into(),
                full_title: None,
                depth: 0,
                kind: NavKind::ReferenceNotice,
                has_children: false,
                is_last: true,
                parent_id: None,
            });
        }
    }
}

fn reference_attachment(
    occurrence: mant_ir::LinkOccurrenceRef<'_, '_>,
    budget: &mut mant_ir::ReferenceWorkBudget,
    location: &ContentLocation,
    valid_owner: bool,
) -> (mant_protocol::ReferenceAttachment, bool) {
    let association = mant_ir::reference_form_associations(occurrence, budget);
    let forms = (valid_owner
        && association.state == mant_ir::ReferenceFormAssociationState::Complete)
        .then_some(association.forms.as_slice());
    (
        mant_protocol::reference_attachment(location, forms),
        matches!(
            association.state,
            mant_ir::ReferenceFormAssociationState::Limited(_)
        ),
    )
}

/// Count physical owners once: index entries do not repeat their native anchor
/// events, and exact section paths retain entries hidden by ambiguous IDs.
struct OwnerCensus<'a> {
    counts: &'a mut HashMap<String, u8>,
    budget: &'a mut mant_ir::ReferenceWorkBudget,
}

impl OwnerCensus<'_> {
    fn count(&mut self, id: &str, depth: usize) -> Result<(), mant_ir::ReferenceScanStop> {
        self.budget.consume(depth, 1, id.len())?;
        if let Some(count) = self.counts.get_mut(id) {
            *count = count.saturating_add(1).min(2);
        }
        Ok(())
    }

    fn entries(
        &mut self,
        entries: &[mant_ir::SemanticEntry],
        depth: usize,
    ) -> Result<(), mant_ir::ReferenceScanStop> {
        for entry in entries {
            self.count(&entry.id, depth)?;
            self.entries(&entry.children, depth.saturating_add(1))?;
        }
        Ok(())
    }

    fn sections(
        &mut self,
        sections: &[mant_ir::Section],
        index: &mant_ir::SemanticIndex,
        path: &mut Vec<usize>,
    ) -> Result<(), mant_ir::ReferenceScanStop> {
        for (position, section) in sections.iter().enumerate() {
            self.count(&section.id, path.len().saturating_add(1))?;
            self.budget.consume(
                path.len(),
                path.len().saturating_add(1),
                std::mem::size_of::<usize>(),
            )?;
            path.push(position);
            self.entries(index.section_at(path), path.len())?;
            self.sections(&section.children, index, path)?;
            path.pop();
        }
        Ok(())
    }
}

fn append_owner_navigation(
    original: &mut std::iter::Peekable<std::vec::IntoIter<NavNode>>,
    output: &mut Vec<NavNode>,
    owners: &BTreeMap<&str, Vec<&ReferenceRecord>>,
) {
    let Some(node) = original.next() else {
        return;
    };
    let depth = node.depth;
    let current = output.len();
    output.push(node);
    let mut last_child = None;
    while original.peek().is_some_and(|node| node.depth > depth) {
        last_child = Some(output.len());
        append_owner_navigation(original, output, owners);
    }
    let Some(records) = owners.get(output[current].id.as_str()) else {
        return;
    };
    output[current].has_children = true;
    if let Some(last) = last_child {
        output[last].is_last = false;
    }
    let group_id = format!("references:{}", output[current].id);
    output.push(NavNode {
        id: group_id.clone(),
        target_id: output[current].target_id.clone(),
        title: format!("DOCUMENT REFERENCES · {}", records.len()),
        full_title: None,
        depth: depth + 1,
        kind: NavKind::ReferenceGroup,
        has_children: true,
        is_last: true,
        parent_id: Some(output[current].id.clone()),
    });
    let mut groups: Vec<Vec<&ReferenceRecord>> = Vec::new();
    let mut keys = HashMap::new();
    for record in records {
        let key = target_key(&record.target);
        let group = *keys.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(record);
    }
    let groups_len = groups.len();
    for (index, records) in groups.into_iter().enumerate() {
        let first = records[0];
        if records.len() == 1 {
            output.push(reference_node(
                first,
                depth + 2,
                index + 1 == groups_len,
                &group_id,
            ));
        } else {
            let target_group = format!("target:{}", first.id);
            output.push(NavNode {
                id: target_group.clone(),
                target_id: first.id.to_string(),
                title: format!(
                    "{} · {} locations",
                    bounded_display(&target_text(&first.target)),
                    records.len()
                ),
                full_title: None,
                depth: depth + 2,
                kind: NavKind::ReferenceGroup,
                has_children: true,
                is_last: index + 1 == groups_len,
                parent_id: Some(group_id.clone()),
            });
            for (index, record) in records.iter().enumerate() {
                output.push(reference_node(
                    record,
                    depth + 3,
                    index + 1 == records.len(),
                    &target_group,
                ));
            }
        }
    }
}

fn display_label(
    occurrence: mant_ir::LinkOccurrenceRef<'_, '_>,
    budget: &mut mant_ir::ReferenceWorkBudget,
    limit: usize,
) -> Option<String> {
    let label =
        mant_ir::reference_label(occurrence.label, occurrence.location.depth(), budget, limit)
            .ok()?;
    if label.text.is_empty() && !label.truncated {
        return Some(format!(
            "{} (unlabelled)",
            bounded_display_limit(&target_text(occurrence.target), limit)
        ));
    }
    let normalized = label.text.replace(['\n', '\r', '\t'], " ");
    let text = crate::text::sanitize_terminal_text(&normalized);
    Some(if label.truncated {
        format!("{text}…")
    } else {
        text.into_owned()
    })
}

fn reference_node(record: &ReferenceRecord, depth: usize, is_last: bool, parent: &str) -> NavNode {
    NavNode {
        id: record.id.to_string(),
        target_id: record.id.to_string(),
        title: format!("↗ {}", record.label),
        full_title: None,
        depth,
        kind: NavKind::Reference,
        has_children: false,
        is_last,
        parent_id: Some(parent.to_owned()),
    }
}

pub(super) fn target_text(target: &LinkTarget) -> String {
    mant_protocol::reference_target_text(target)
}

fn target_bytes(target: &LinkTarget) -> usize {
    match target {
        LinkTarget::Document { name, fragment } => name
            .len()
            .saturating_add(fragment.as_ref().map_or(0, String::len)),
        LinkTarget::Manual {
            name,
            manual_section,
        } => name
            .len()
            .saturating_add(manual_section.as_ref().map_or(0, String::len)),
        LinkTarget::Section { id } => id.as_str().len(),
        LinkTarget::External { uri } => uri.len(),
        LinkTarget::Email { address } => address.len(),
    }
}

fn bounded_display(value: &str) -> String {
    bounded_display_limit(value, MAX_LABEL)
}

fn bounded_display_limit(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let normalized = value[..end].replace(['\n', '\r', '\t'], " ");
    let text = crate::text::sanitize_terminal_text(&normalized);
    if end == value.len() {
        text.into_owned()
    } else {
        format!("{text}…")
    }
}

// Length framing preserves full typed-target equality without Debug escaping
// or conflating absent fragments with empty fragments in unchecked input IR.
fn target_key(target: &LinkTarget) -> String {
    match target {
        LinkTarget::Document { name, fragment } => format!(
            "d{}:{name}{}",
            name.len(),
            optional_key(fragment.as_deref())
        ),
        LinkTarget::Manual {
            name,
            manual_section,
        } => format!(
            "m{}:{name}{}",
            name.len(),
            optional_key(manual_section.as_deref())
        ),
        _ => unreachable!("the inventory only retains document and manual references"),
    }
}

fn optional_key(value: Option<&str>) -> String {
    value.map_or_else(|| "n".into(), |value| format!("s{}:{value}", value.len()))
}
