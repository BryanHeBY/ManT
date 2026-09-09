//! Bounded association with original form slices, without form reconstruction.

use crate::{
    ContentBlockStep, ContentInlineRoot, ContentLocationRef, EntryContentSlice, EntryInlineRoot,
    EntryOwner, Inline,
};

use super::{LinkOccurrenceRef, ReferenceScanStop, ReferenceWorkBudget};

/// Maximum form ordinals retained for one link (four KiB of coordinates).
pub const MAX_REFERENCE_FORM_ASSOCIATIONS: usize = 1024;

/// Coverage of optional semantic-form association, independent from link coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceFormAssociationState {
    /// No semantic owner or no recorded forms; not an inferred negative claim.
    Unrecorded,
    /// All original forms were checked; the returned ordinals are complete.
    Complete,
    /// An original form binding was invalid; no partial association survives.
    Invalid,
    /// Shared traversal/inspection work was exhausted; no associations returned.
    Limited(ReferenceScanStop),
    /// The bounded ordinal array would exceed its materialization limit.
    MaterializationLimit,
}

/// Validated source-form ordinals referring to this one original link occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceFormAssociations {
    /// Association coverage, never a statement about target resolution.
    pub state: ReferenceFormAssociationState,
    /// Original owner-local form ordinals; these do not create extra links.
    pub forms: Vec<u32>,
}

/// Validate original forms and associate their slices with an existing link.
///
/// The budget is the scan's shared account, not a fresh budget per occurrence.
/// Invalid or limited work rejects the association set atomically. Original
/// links remain available even when their owner's semantic metadata is wrong.
#[must_use]
pub fn reference_form_associations(
    occurrence: LinkOccurrenceRef<'_, '_>,
    budget: &mut ReferenceWorkBudget,
) -> ReferenceFormAssociations {
    let Some(context) = occurrence.semantic_owner else {
        return empty(ReferenceFormAssociationState::Unrecorded);
    };
    let Some(facts) = context
        .owner
        .facts()
        .filter(|facts| !facts.forms.is_empty())
    else {
        return empty(ReferenceFormAssociationState::Unrecorded);
    };
    let depth = context
        .location
        .sections
        .len()
        .saturating_add(context.location.blocks.len());
    if let Err(reason) = budget.consume(
        depth,
        depth.saturating_add(1),
        depth.saturating_mul(std::mem::size_of::<u32>()),
    ) {
        return empty(ReferenceFormAssociationState::Limited(reason));
    }
    let relative = local_link_root(occurrence);
    let mut forms = Vec::new();
    for (index, form) in facts.forms.iter().enumerate() {
        if let Err(reason) = budget.consume(depth, 1, 0) {
            return empty(ReferenceFormAssociationState::Limited(reason));
        }
        if form.parts.is_empty() {
            return empty(ReferenceFormAssociationState::Invalid);
        }
        let mut previous: Option<&EntryContentSlice> = None;
        let mut associated = false;
        for part in &form.parts {
            let work = part.path.len().saturating_mul(3).saturating_add(1);
            let bytes = part
                .path
                .len()
                .saturating_mul(std::mem::size_of::<usize>())
                .saturating_add(32);
            if let Err(reason) = budget.consume(
                depth.saturating_add(part.path.len()).saturating_add(
                    usize::from(matches!(part.root, EntryInlineRoot::Block { .. })) * 2,
                ),
                work,
                bytes,
            ) {
                return empty(ReferenceFormAssociationState::Limited(reason));
            }
            if previous.is_some_and(|left| !left.precedes(part))
                || !valid_slice(context.owner, part)
            {
                return empty(ReferenceFormAssociationState::Invalid);
            }
            if let Some((root, path)) = &relative {
                associated |= *root == part.root && overlapping_paths(&part.path, path);
            }
            previous = Some(part);
        }
        if associated {
            if forms.len() == MAX_REFERENCE_FORM_ASSOCIATIONS {
                return empty(ReferenceFormAssociationState::MaterializationLimit);
            }
            let Ok(index) = u32::try_from(index) else {
                return empty(ReferenceFormAssociationState::Invalid);
            };
            forms.push(index);
        }
    }
    ReferenceFormAssociations {
        state: ReferenceFormAssociationState::Complete,
        forms,
    }
}

fn empty(state: ReferenceFormAssociationState) -> ReferenceFormAssociations {
    ReferenceFormAssociations {
        state,
        forms: Vec::new(),
    }
}

fn local_link_root<'a>(
    occurrence: LinkOccurrenceRef<'_, 'a>,
) -> Option<(EntryInlineRoot, &'a [u32])> {
    let context = occurrence.semantic_owner?;
    let ContentLocationRef::Content {
        sections,
        blocks,
        root,
        path,
    } = occurrence.location
    else {
        return None;
    };
    if sections != context.location.sections {
        return None;
    }
    if blocks == context.location.blocks {
        if let ContentInlineRoot::DefinitionTerm {
            item_index,
            term_index,
        } = root
            && item_index == context.location.item_index
            && matches!(context.owner, EntryOwner::Definition(_))
        {
            return Some((
                EntryInlineRoot::Term {
                    index: term_index as usize,
                },
                path,
            ));
        }
        return None;
    }
    let trailing = blocks.strip_prefix(context.location.blocks)?;
    let [step, ContentBlockStep::Block { index }] = trailing else {
        return None;
    };
    let correct_item = match (context.owner, step) {
        (EntryOwner::List(_), ContentBlockStep::ListItem { index })
        | (EntryOwner::Definition(_), ContentBlockStep::DefinitionItem { index }) => {
            *index == context.location.item_index
        }
        _ => false,
    };
    (correct_item && root == ContentInlineRoot::Inlines).then_some((
        EntryInlineRoot::Block {
            index: *index as usize,
        },
        path,
    ))
}

fn overlapping_paths(part: &[usize], link: &[u32]) -> bool {
    // A form can select the link itself, an ancestor containing it, or code
    // inside it. Slice reconstruction preserves link wrappers in the last case.
    part.iter()
        .zip(link)
        .all(|(left, right)| *left == *right as usize)
}

fn valid_slice(owner: EntryOwner<'_>, part: &EntryContentSlice) -> bool {
    let Some(mut nodes) = owner.inline_root(&part.root) else {
        return false;
    };
    for (depth, index) in part.path.iter().enumerate() {
        let Some(node) = nodes.get(*index) else {
            return false;
        };
        nodes = if depth + 1 == part.path.len() {
            std::slice::from_ref(node)
        } else {
            let Some(children) = crate::content_location::inline_children(node) else {
                return false;
            };
            children
        };
    }
    if let Some(bytes) = &part.bytes {
        if part.path.is_empty() || bytes.start >= bytes.end {
            return false;
        }
        let [Inline::Text { value } | Inline::Code { value }] = nodes else {
            return false;
        };
        value.get(bytes.clone()).is_some()
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, ReferenceScanLimits, ReferenceScope, scan_reference_scope};
    use serde_json::json;
    use std::ops::ControlFlow;

    fn document(forms: serde_json::Value) -> Document {
        let mut value = json!({"parser":null,"source":{"format":"markdown"},"meta":{},"sections":[],"blocks":[{
            "type":"list","kind":{"kind":"bullet"},"items":[{
                "entry":{"id":"entry","kind":{"kind":"command"},"case":"sensitive","names":[],"valueDomain":null},
                "blocks":[{"type":"paragraph","children":[
                    {"type":"link","target":{"kind":"document","name":"target"},"children":[{"type":"code","value":"étool"}]},
                    {"type":"text","value":" see "},
                    {"type":"link","target":{"kind":"document","name":"body"},"children":[]}
                ]}]
            }]
        }]});
        value["blocks"][0]["items"][0]["entry"]["forms"] = forms;
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn original_form_slices_associate_without_duplicate_links_or_name_validation() {
        let document = document(json!([
            {"parts":[{"root":{"kind":"block","index":0},"path":[0]}]},
            {"parts":[{"root":{"kind":"block","index":0},"path":[0,0],"bytes":{"start":0,"end":2}}]}
        ]));
        let mut records = Vec::new();
        let report = scan_reference_scope(
            &document,
            ReferenceScope::Document,
            ReferenceScanLimits::default(),
            |occurrence, budget| {
                records.push(reference_form_associations(occurrence, budget));
                ControlFlow::Continue(())
            },
        );
        assert!(report.complete());
        assert_eq!(report.occurrences, 2);
        assert_eq!(records[0].forms, [0, 1]);
        assert!(records[1].forms.is_empty());
        assert!(
            records
                .iter()
                .all(|record| record.state == ReferenceFormAssociationState::Complete)
        );
    }

    #[test]
    fn malformed_later_form_removes_earlier_association_but_not_visible_links() {
        for path in [json!([99]), json!([0, 0, 0])] {
            let document = document(json!([
                {"parts":[{"root":{"kind":"block","index":0},"path":[0]}]},
                {"parts":[{"root":{"kind":"block","index":0},"path":path}]}
            ]));
            let report = scan_reference_scope(
                &document,
                ReferenceScope::Document,
                ReferenceScanLimits::default(),
                |occurrence, budget| {
                    let result = reference_form_associations(occurrence, budget);
                    assert_eq!(result.state, ReferenceFormAssociationState::Invalid);
                    assert!(result.forms.is_empty());
                    ControlFlow::Continue(())
                },
            );
            assert!(report.complete());
            assert_eq!(report.occurrences, 2);
        }
    }

    #[test]
    fn form_checks_spend_the_same_budget_as_subsequent_scan_work() {
        let document = document(json!(
            (0..100)
                .map(|_| json!({"parts":[{"root":{"kind":"block","index":0},"path":[0]}]}))
                .collect::<Vec<_>>()
        ));
        let mut limited = false;
        let report = scan_reference_scope(
            &document,
            ReferenceScope::Document,
            ReferenceScanLimits {
                steps: 20,
                ..Default::default()
            },
            |occurrence, budget| {
                let result = reference_form_associations(occurrence, budget);
                limited |= matches!(
                    result.state,
                    ReferenceFormAssociationState::Limited(ReferenceScanStop::Steps)
                );
                assert!(result.forms.is_empty());
                ControlFlow::Continue(())
            },
        );
        assert!(limited);
        assert!(report.steps <= 20);
        assert_eq!(report.stopped, Some(ReferenceScanStop::Steps));
    }
}
