//! Address derivation and one bounded exact-target pass, never target loading.

use mant_ir::{
    ContentReveal, Document, DocumentAddress, DocumentReference, LinkTarget, NavigationEvent,
    NavigationScanOptions, ReferenceLinkFilter, ReferenceScope, ReferenceWorkBudget,
    scan_navigation_scope_with_budget,
};
use mant_protocol::{
    LoadedFragment, ReferenceCoverage, ReferenceRecord, ReferenceResolution, UnloadedFragment,
};
use std::{collections::BTreeMap, ops::ControlFlow};

pub(super) fn initial(
    target: &LinkTarget,
    source: Option<&DocumentAddress>,
) -> ReferenceResolution {
    match target {
        LinkTarget::External { .. } | LinkTarget::Email { .. } => {
            ReferenceResolution::NotApplicable {}
        }
        LinkTarget::Section { .. } => ReferenceResolution::Loaded {
            address: source.cloned(),
            fragment: LoadedFragment::Limited {},
        },
        LinkTarget::Document { name, fragment } => {
            let unloaded = if fragment.is_some() {
                UnloadedFragment::Unchecked {}
            } else {
                UnloadedFragment::Absent {}
            };
            let reference = DocumentReference::Document {
                name: name.clone(),
                fragment: fragment.clone(),
            };
            if !reference.is_well_formed() {
                return ReferenceResolution::Restricted {};
            }
            let Some(source) = source else {
                return ReferenceResolution::MissingContext { fragment: unloaded };
            };
            let Some(address) = reference.resolve_from(source) else {
                return ReferenceResolution::Restricted {};
            };
            if address == *source {
                ReferenceResolution::Loaded {
                    address: Some(address),
                    fragment: if fragment.is_some() {
                        LoadedFragment::Limited {}
                    } else {
                        LoadedFragment::Absent {}
                    },
                }
            } else {
                ReferenceResolution::LogicalAddress {
                    address,
                    fragment: unloaded,
                }
            }
        }
        LinkTarget::Manual {
            name,
            manual_section,
        } => {
            let reference = DocumentReference::Manual {
                name: name.clone(),
                manual_section: manual_section.clone(),
            };
            if !reference.is_well_formed() {
                return ReferenceResolution::Restricted {};
            }
            let Some(section) = manual_section else {
                return ReferenceResolution::NotQueried {
                    fragment: UnloadedFragment::Absent {},
                };
            };
            let address = DocumentAddress::Manual {
                name: name.clone(),
                manual_section: section.clone(),
            };
            if source == Some(&address) {
                ReferenceResolution::Loaded {
                    address: Some(address),
                    fragment: LoadedFragment::Absent {},
                }
            } else {
                ReferenceResolution::LogicalAddress {
                    address,
                    fragment: UnloadedFragment::Absent {},
                }
            }
        }
    }
}

#[derive(Default)]
enum Match {
    #[default]
    Missing,
    Found(ContentReveal),
    Ambiguous,
    Limited,
}

fn requested(record: &ReferenceRecord) -> Option<&str> {
    if !matches!(
        record.resolution,
        ReferenceResolution::Loaded {
            fragment: LoadedFragment::Limited {},
            ..
        }
    ) {
        return None;
    }
    match &record.target {
        LinkTarget::Section { id } => Some(id.as_str()),
        LinkTarget::Document { fragment, .. } => fragment.as_deref(),
        _ => None,
    }
}

pub(super) fn validate_local(
    document: &Document,
    _source: Option<&DocumentAddress>,
    records: &mut [ReferenceRecord],
    budget: &mut ReferenceWorkBudget,
    retained: &mut usize,
    limit: usize,
) -> Option<ReferenceCoverage> {
    let mut wanted: BTreeMap<&str, Match> = BTreeMap::new();
    for record in records.iter() {
        if let Some(fragment) = requested(record) {
            wanted.entry(fragment).or_default();
        }
    }
    if wanted.is_empty() {
        return None;
    }
    let report = scan_navigation_scope_with_budget(
        document,
        ReferenceScope::Document,
        budget,
        NavigationScanOptions {
            links: ReferenceLinkFilter::NONE,
            targets: true,
            entry_sets: false,
        },
        |event, budget| {
            let NavigationEvent::Target(target) = event else {
                return ControlFlow::Continue(());
            };
            inspect_target(target, &mut wanted, budget, retained, limit)
        },
    );
    // Move borrowed lookup outcomes out before mutating the owning records.
    // Only one result per retained record is created and each copied reveal is
    // charged against the same record budget before allocation.
    let results: Vec<_> = records
        .iter()
        .map(|record| {
            let name = requested(record)?;
            Some(materialize_match(
                wanted.get(name),
                report.complete(),
                budget,
                retained,
                limit,
            ))
        })
        .collect();
    drop(wanted);
    for (record, result) in records.iter_mut().zip(results) {
        if let Some(fragment) = result
            && let ReferenceResolution::Loaded {
                fragment: current, ..
            } = &mut record.resolution
        {
            *current = fragment;
        }
    }
    Some(ReferenceCoverage::from_report(
        mant_ir::ReferenceScanReport {
            steps: budget.used_steps(),
            bytes: budget.used_bytes(),
            stopped: budget.stopped().or(report.stopped),
            ..report
        },
    ))
}

fn inspect_target(
    target: mant_ir::NavigationTargetRef<'_, '_>,
    wanted: &mut BTreeMap<&str, Match>,
    budget: &mut ReferenceWorkBudget,
    retained: &mut usize,
    limit: usize,
) -> ControlFlow<()> {
    for name in std::iter::once(target.id.as_str())
        .chain(target.aliases.iter().map(mant_ir::FragmentAlias::as_str))
    {
        if budget
            .consume(target.reveal.depth(), 1, name.len().saturating_mul(12))
            .is_err()
        {
            return ControlFlow::Break(());
        }
        let Some(found) = wanted.get_mut(name) else {
            continue;
        };
        if matches!(found, Match::Ambiguous | Match::Limited) {
            continue;
        }
        if let Match::Found(previous) = found {
            if budget
                .consume(target.reveal.depth(), target.reveal.depth(), 0)
                .is_err()
            {
                return ControlFlow::Break(());
            }
            if previous.as_ref() != target.reveal {
                *found = Match::Ambiguous;
            }
            continue;
        }
        if budget
            .consume(
                target.reveal.depth(),
                target.reveal.depth().saturating_mul(3),
                0,
            )
            .is_err()
        {
            *found = Match::Limited;
            return ControlFlow::Break(());
        }
        let bytes = target.reveal.encoded_size_bound();
        if bytes > limit.saturating_sub(*retained) {
            *found = Match::Limited;
            continue;
        }
        let Some(reveal) = target.reveal.to_owned() else {
            *found = Match::Limited;
            continue;
        };
        *found = Match::Found(reveal);
        *retained += bytes;
    }
    ControlFlow::Continue(())
}

fn materialize_match(
    found: Option<&Match>,
    complete: bool,
    budget: &mut ReferenceWorkBudget,
    retained: &mut usize,
    limit: usize,
) -> LoadedFragment {
    match found {
        Some(Match::Ambiguous) => LoadedFragment::Ambiguous {},
        _ if !complete => LoadedFragment::Limited {},
        Some(Match::Found(reveal)) => {
            let borrowed = reveal.as_ref();
            if budget
                .consume(borrowed.depth(), borrowed.depth().saturating_mul(2), 0)
                .is_err()
            {
                return LoadedFragment::Limited {};
            }
            let bytes = borrowed.encoded_size_bound();
            if bytes > limit.saturating_sub(*retained) {
                LoadedFragment::Limited {}
            } else {
                *retained += bytes;
                LoadedFragment::Valid {
                    reveal: reveal.clone(),
                }
            }
        }
        Some(Match::Missing) | None => LoadedFragment::Missing {},
        Some(Match::Limited) => LoadedFragment::Limited {},
    }
}
