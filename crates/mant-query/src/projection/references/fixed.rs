//! Bounded reference inventory over checked Fixed marks, never a Flow projection.

use std::{collections::BTreeSet, num::NonZeroU32};

use mant_ir::{
    DocumentAddress, FixedBody, LinkMark, ReferenceLinkFilter, ReferenceScanReport,
    ReferenceWorkBudget, TextJoin,
};
use mant_protocol::{
    ContentSelector, FixedReferenceOrigin, FixedReferenceRecord, ReferenceInventory,
    ReferencePageLimit, ReferenceProjection, ReferenceProjectionMode,
};

use super::{
    ReferenceProjectionLimits, finish_inventory, resolution, serialized_size, target_key,
    target_size,
};

/// A selected source subtree, resolved from the same Fixed outline snapshot.
#[derive(Debug, Clone, Copy)]
pub(in crate::projection) enum Scope {
    Document,
    Overview,
    Section(NonZeroU32),
    Owner(NonZeroU32),
}

/// Scan native link instances in identity order. The caller has already
/// validated the Fixed body while building its outline, so this operation
/// charges only the additional scope, target and preview work it performs.
pub(in crate::projection) fn project(
    fixed: &FixedBody,
    source_address: Option<&DocumentAddress>,
    scope: Scope,
    policy: &ReferenceProjection,
    mut limits: ReferenceProjectionLimits,
) -> ReferenceInventory {
    limits.distinct_targets = limits.distinct_targets.min(4096);
    limits.distinct_bytes = limits.distinct_bytes.min(1024 * 1024);
    limits.materialization_bytes = limits.materialization_bytes.min(1024 * 1024);
    let mut result = ReferenceInventory::not_scanned(policy.clone());
    if policy.mode == ReferenceProjectionMode::None || policy.validate().is_err() {
        return result;
    }
    let mut budget = ReferenceWorkBudget::new(limits.scan);
    let scope_steps = match scope {
        Scope::Section(_) => fixed.headings.len().saturating_add(1),
        Scope::Owner(_) => fixed.owners.len().saturating_add(1),
        Scope::Document | Scope::Overview => 0,
    };
    if budget.consume(0, scope_steps, 0).is_err() {
        finish_fixed(&mut result, &budget, 0, 0, true);
        return result;
    }
    let (section_scope, owner_scope) = scope_masks(fixed, scope);
    let filter = ReferenceLinkFilter::from_types(&policy.target_types);
    let mut targets = BTreeSet::new();
    let mut target_bytes = 0usize;
    let mut targets_complete = true;
    let mut occurrences = 0usize;
    let mut retained = 0usize;
    for link in &fixed.links {
        if budget.consume(0, 1, 0).is_err() {
            break;
        }
        let Some(target) = link.target.as_ref() else {
            continue;
        };
        if !filter.contains(target)
            || !in_scope(
                link,
                scope,
                section_scope.as_deref(),
                owner_scope.as_deref(),
            )
        {
            continue;
        }
        let target_len = target_size(target);
        if budget.consume(0, 1, target_len.saturating_mul(16)).is_err() {
            break;
        }
        let index = occurrences;
        occurrences += 1;
        if targets_complete {
            let key = target_key(target);
            if !targets.contains(&key) {
                let bytes = target_len.saturating_add(32);
                if targets.len() >= limits.distinct_targets
                    || bytes > limits.distinct_bytes.saturating_sub(target_bytes)
                {
                    targets_complete = false;
                } else {
                    targets.insert(key);
                    target_bytes += bytes;
                }
            }
        }
        if policy.mode != ReferenceProjectionMode::All
            || index < policy.offset as usize
            || result.page.limited.is_some()
        {
            continue;
        }
        if result.fixed_records.len() >= policy.limit as usize {
            result.page.limited = Some(ReferencePageLimit::Records);
            continue;
        }
        match materialize(
            fixed,
            link,
            source_address,
            &mut budget,
            retained,
            limits.materialization_bytes,
        ) {
            Ok((record, bytes)) => {
                retained += bytes;
                result.fixed_records.push(record);
            }
            Err(limit) => result.page.limited = Some(limit),
        }
    }
    finish_fixed(
        &mut result,
        &budget,
        occurrences,
        targets.len(),
        targets_complete,
    );
    result
}

fn scope_masks(fixed: &FixedBody, scope: Scope) -> (Option<Vec<bool>>, Option<Vec<bool>>) {
    let section_scope = matches!(scope, Scope::Section(_)).then(|| {
        let mut selected = vec![false; fixed.headings.len() + 1];
        for heading in &fixed.headings {
            let key = heading.key.get() as usize;
            selected[key] = matches!(scope, Scope::Section(wanted) if heading.key == wanted)
                || heading
                    .parent
                    .is_some_and(|parent| selected[parent.get() as usize]);
        }
        selected
    });
    let owner_scope = matches!(scope, Scope::Owner(_)).then(|| {
        let mut selected = vec![false; fixed.owners.len() + 1];
        for owner in &fixed.owners {
            let key = owner.key.get() as usize;
            selected[key] = matches!(scope, Scope::Owner(wanted) if owner.key == wanted)
                || owner
                    .parent
                    .is_some_and(|parent| selected[parent.get() as usize]);
        }
        selected
    });
    (section_scope, owner_scope)
}

fn in_scope(
    link: &LinkMark,
    scope: Scope,
    sections: Option<&[bool]>,
    owners: Option<&[bool]>,
) -> bool {
    match scope {
        Scope::Document => true,
        Scope::Overview => link.section.is_none(),
        Scope::Section(_) => link
            .section
            .is_some_and(|key| sections.is_some_and(|selected| selected[key.get() as usize])),
        Scope::Owner(_) => link
            .owner
            .is_some_and(|key| owners.is_some_and(|selected| selected[key.get() as usize])),
    }
}

fn materialize(
    fixed: &FixedBody,
    link: &LinkMark,
    source_address: Option<&DocumentAddress>,
    budget: &mut ReferenceWorkBudget,
    retained: usize,
    limit: usize,
) -> Result<(FixedReferenceRecord, usize), ReferencePageLimit> {
    let target = link.target.as_ref().ok_or(ReferencePageLimit::Position)?;
    let target_len = target_size(target);
    let available = limit.saturating_sub(retained);
    if target_len.saturating_mul(3).saturating_add(512) > available {
        return Err(ReferencePageLimit::MaterializationBytes);
    }
    budget
        .consume(0, 1, target_len)
        .map_err(|_| ReferencePageLimit::Scan)?;
    let (label_preview, label_preview_truncated) = preview(fixed, link, budget)?;
    let source_read = link.section.map_or_else(
        || ContentSelector::path("root"),
        |key| ContentSelector::id(fixed.headings[(key.get() - 1) as usize].id.clone()),
    );
    let record = FixedReferenceRecord {
        origin: FixedReferenceOrigin {
            link: link.key,
            section: link.section,
            owner: link.owner,
            first_slice: link.label.parts.first().copied(),
            source: link.source,
            source_key: link.source_key,
        },
        source_read,
        target: target.clone(),
        label_preview,
        label_preview_truncated,
        resolution: resolution::initial(target, source_address),
    };
    let bytes =
        serialized_size(&record, available).ok_or(ReferencePageLimit::MaterializationBytes)?;
    Ok((record, bytes))
}

fn preview(
    fixed: &FixedBody,
    link: &LinkMark,
    budget: &mut ReferenceWorkBudget,
) -> Result<(String, bool), ReferencePageLimit> {
    const LIMIT: usize = 4096;
    let mut text = String::new();
    for (index, part) in link.label.parts.iter().enumerate() {
        if index != 0 {
            let separator = match &link.label.joins[index - 1] {
                TextJoin::AuthoredSeparator(value) | TextJoin::GeneratedSeparator(value) => {
                    value.as_str()
                }
                TextJoin::DirectContact | TextJoin::HardBoundary | TextJoin::Unknown => "",
            };
            if append(&mut text, separator, budget, LIMIT)? {
                return Ok((text, true));
            }
        }
        let run = fixed
            .surface
            .run_text(part.run)
            .ok_or(ReferencePageLimit::Position)?;
        let start = usize::try_from(part.start_byte).map_err(|_| ReferencePageLimit::Position)?;
        let end = usize::try_from(part.end_byte).map_err(|_| ReferencePageLimit::Position)?;
        let slice = run.get(start..end).ok_or(ReferencePageLimit::Position)?;
        if append(&mut text, slice, budget, LIMIT)? {
            return Ok((text, true));
        }
    }
    Ok((text, false))
}

fn append(
    result: &mut String,
    source: &str,
    budget: &mut ReferenceWorkBudget,
    limit: usize,
) -> Result<bool, ReferencePageLimit> {
    let remaining = limit.saturating_sub(result.len());
    let mut end = source.len().min(remaining);
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    budget
        .consume(0, 1, end)
        .map_err(|_| ReferencePageLimit::Scan)?;
    result.push_str(&source[..end]);
    Ok(end < source.len())
}

fn finish_fixed(
    result: &mut ReferenceInventory,
    budget: &ReferenceWorkBudget,
    occurrences: usize,
    targets: usize,
    targets_complete: bool,
) {
    finish_inventory(
        result,
        ReferenceScanReport {
            steps: budget.used_steps(),
            bytes: budget.used_bytes(),
            occurrences,
            stopped: budget.stopped(),
        },
        occurrences,
        targets,
        targets_complete,
    );
}
