//! Budgeted occurrence projection; never loads destinations or builds `SemanticIndex`.

pub(super) mod fixed;
mod resolution;
#[cfg(test)]
mod tests;

use mant_ir::{
    ContentProjectionBuilder, ContentRevealRef, Document, DocumentAddress, LinkOccurrenceRef,
    LinkTarget, NavigationEvent, NavigationScanOptions, ReferenceFormAssociationState,
    ReferenceLinkFilter, ReferenceScanLimits, ReferenceScope, ReferenceTargetType,
    ReferenceWorkBudget, reference_form_associations, scan_navigation_scope_with_budget,
};
use mant_protocol::{
    ReferenceAssociation, ReferenceCount, ReferenceCoverage, ReferenceInventory,
    ReferencePageLimit, ReferenceProjection, ReferenceProjectionMode, ReferenceRecord,
    ReferenceUnknownReason,
};
use std::{collections::BTreeSet, ops::ControlFlow};

/// Independent reference operation budgets; input-file size is not a substitute.
#[derive(Debug, Clone, Copy)]
pub struct ReferenceProjectionLimits {
    /// Shared traversal, label, form and local-destination inspection limits.
    pub scan: ReferenceScanLimits,
    /// Distinct typed-target keys; clamped to 4,096.
    pub distinct_targets: usize,
    /// Total retained distinct key text/metadata; clamped to 1 MiB.
    pub distinct_bytes: usize,
    /// Total materialized records/positions/labels; default 256 KiB, ceiling 1 MiB.
    pub materialization_bytes: usize,
}

impl Default for ReferenceProjectionLimits {
    fn default() -> Self {
        Self {
            scan: ReferenceScanLimits::default(),
            distinct_targets: 4096,
            distinct_bytes: 1024 * 1024,
            materialization_bytes: 256 * 1024,
        }
    }
}

/// Project selected original links without catalog I/O or implicit document loading.
/// Invalid policies return an unscanned inventory; process boundaries validate first.
#[must_use]
pub fn project_references(
    document: &Document,
    source_address: Option<&DocumentAddress>,
    scope: ReferenceScope<'_>,
    policy: &ReferenceProjection,
) -> ReferenceInventory {
    project_references_with_limits(
        document,
        source_address,
        scope,
        policy,
        ReferenceProjectionLimits::default(),
    )
}

/// Project under explicit hard-clamped budgets, including all skipped offset work.
///
/// # Panics
///
/// Only if an admitted link is absent from its completed projection, which
/// violates the internal closure invariant.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn project_references_with_limits(
    document: &Document,
    source_address: Option<&DocumentAddress>,
    scope: ReferenceScope<'_>,
    policy: &ReferenceProjection,
    mut limits: ReferenceProjectionLimits,
) -> ReferenceInventory {
    limits.distinct_targets = limits.distinct_targets.min(4096);
    limits.distinct_bytes = limits.distinct_bytes.min(1024 * 1024);
    limits.materialization_bytes = limits.materialization_bytes.min(1024 * 1024);
    if policy.validate().is_err() {
        return invalid_policy(policy);
    }
    let mut result = ReferenceInventory::not_scanned(policy.clone());
    if policy.mode == ReferenceProjectionMode::None {
        return result;
    }
    // This infallible v0.12 API carries no Fixed reference coordinates. Do not
    // turn a Fixed page into a complete zero-occurrence Flow inventory.
    let Some(_flow) = document.flow() else {
        return result;
    };
    let mut budget = ReferenceWorkBudget::new(limits.scan);
    let mut targets = BTreeSet::new();
    let mut target_bytes = 0usize;
    let mut targets_complete = true;
    let mut occurrences = 0usize;
    let mut retained_bytes = 0usize;
    let mut record_bytes = Vec::new();
    let source_bytes = source_address.map_or(0, address_bytes);
    let content = document.content();
    let report = scan_navigation_scope_with_budget(
        document,
        scope,
        &mut budget,
        NavigationScanOptions {
            links: ReferenceLinkFilter::from_types(&policy.target_types),
            ..Default::default()
        },
        |event, budget| {
            let NavigationEvent::Link(occurrence) = event else {
                return ControlFlow::Continue(());
            };
            let index = occurrences;
            occurrences += 1;
            if targets_complete {
                // At most 4,096 keys: reserve bounded comparisons and insert
                // inspection, not just the single scanner visit to this text.
                if budget
                    .consume(
                        occurrence.location.depth(),
                        1,
                        target_size(occurrence.target).saturating_mul(16),
                    )
                    .is_err()
                {
                    return ControlFlow::Break(());
                }
                let key = target_key(occurrence.target);
                if !targets.contains(&key) {
                    let bytes = target_size(occurrence.target).saturating_add(32);
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
                || index < (policy.offset as usize)
                || result.page.limited.is_some()
            {
                return ControlFlow::Continue(());
            }
            if result.records.len() >= policy.limit as usize {
                result.page.limited = Some(ReferencePageLimit::Records);
                return ControlFlow::Continue(());
            }
            let before = retained_bytes;
            match materialize(
                content,
                &occurrence,
                source_address,
                source_bytes,
                budget,
                &mut retained_bytes,
                limits.materialization_bytes,
            ) {
                Ok(record) => {
                    record_bytes.push(retained_bytes - before);
                    result.records.push(record);
                }
                Err(limit) => result.page.limited = Some(limit),
            }
            ControlFlow::Continue(())
        },
    );
    let remap = attach_projection(
        document,
        &mut result,
        &record_bytes,
        &mut retained_bytes,
        limits.materialization_bytes,
    );
    result.target_coverage = resolution::validate_local(
        document,
        source_address,
        &mut result.records,
        &mut budget,
        &mut retained_bytes,
        limits.materialization_bytes,
    );
    if let Some(remap) = remap {
        // Local target validation uses original occurrence keys; remap only
        // after that pass has finished reading the document's source store.
        for record in &mut result.records {
            remap
                .remap_link(&mut record.occurrence)
                .expect("admitted link belongs to the completed projection");
        }
    }
    finish_inventory(
        &mut result,
        report,
        occurrences,
        targets.len(),
        targets_complete,
    );
    result
}

fn invalid_policy(policy: &ReferenceProjection) -> ReferenceInventory {
    // Never clone an invalid caller-provided, potentially huge type vector.
    let mut invalid = ReferenceInventory::not_scanned(ReferenceProjection {
        mode: policy.mode,
        target_types: Vec::new(),
        offset: policy.offset,
        limit: policy.limit.clamp(1, 1000),
    });
    invalid.occurrences = ReferenceCount::Unknown {
        reason: ReferenceUnknownReason::InvalidPolicy,
    };
    invalid.targets = ReferenceCount::Unknown {
        reason: ReferenceUnknownReason::InvalidPolicy,
    };
    invalid
}

fn finish_inventory(
    result: &mut ReferenceInventory,
    report: mant_ir::ReferenceScanReport,
    occurrences: usize,
    targets: usize,
    targets_complete: bool,
) {
    let count = |value, exact| {
        if exact {
            ReferenceCount::Exact { value }
        } else {
            ReferenceCount::LowerBound { value }
        }
    };
    result.occurrences = count(occurrences as u64, report.complete());
    result.targets = count(targets as u64, report.complete() && targets_complete);
    result.coverage = ReferenceCoverage::from_report(report);
    result.page.returned = u32::try_from(
        result
            .records
            .len()
            .saturating_add(result.fixed_records.len()),
    )
    .unwrap_or(u32::MAX);
    if !report.complete()
        && result.policy.mode == ReferenceProjectionMode::All
        && result.page.limited.is_none()
    {
        result.page.limited = Some(ReferencePageLimit::Scan);
    }
    let next = result.policy.offset.saturating_add(result.page.returned);
    // Only a completed scan proves that a fresh operation can reach the next
    // source position; zero-progress materialization never advertises a loop.
    if report.complete() && result.page.returned > 0 && (next as usize) < occurrences {
        result.page.next_offset = Some(next);
    }
}

fn materialize(
    content: mant_ir::ContentContext<'_>,
    occurrence: &LinkOccurrenceRef<'_, '_>,
    source_address: Option<&DocumentAddress>,
    source_bytes: usize,
    budget: &mut ReferenceWorkBudget,
    retained: &mut usize,
    limit: usize,
) -> Result<ReferenceRecord, ReferencePageLimit> {
    let owner = occurrence
        .content_owner
        .map(|owner| ContentRevealRef::Owner(owner.location));
    let semantic_owner = occurrence
        .semantic_owner
        .map(|owner| ContentRevealRef::Owner(owner.location));
    budget
        .consume(
            occurrence.location.depth(),
            occurrence
                .location
                .depth()
                .saturating_add(owner.map_or(0, ContentRevealRef::depth))
                .saturating_add(semantic_owner.map_or(0, ContentRevealRef::depth))
                .saturating_mul(4),
            source_bytes,
        )
        .map_err(|_| ReferencePageLimit::Scan)?;
    let owner_bytes = owner.map_or(0, ContentRevealRef::encoded_size_bound);
    let forms_reservation = occurrence
        .semantic_owner
        .and_then(|owner| owner.owner.facts())
        .map_or(0, |facts| {
            facts
                .forms
                .len()
                .min(mant_ir::MAX_REFERENCE_FORM_ASSOCIATIONS)
                .saturating_mul(12)
        });
    let bytes = occurrence
        .location
        .encoded_len()
        .saturating_mul(2)
        .saturating_add(owner_bytes)
        .saturating_add(semantic_owner.map_or(0, ContentRevealRef::encoded_size_bound))
        .saturating_add(source_bytes.saturating_mul(2))
        .saturating_add(forms_reservation)
        .saturating_add(256);
    if bytes > limit.saturating_sub(*retained) {
        return Err(ReferencePageLimit::MaterializationBytes);
    }
    let origin = occurrence
        .location
        .to_owned()
        .ok_or(ReferencePageLimit::Position)?;
    let owner = match owner {
        Some(owner) => Some(owner.to_owned().ok_or(ReferencePageLimit::Position)?),
        None => None,
    };
    let label = content
        .reference_occurrence_label(
            occurrence.key,
            occurrence.location.depth(),
            budget,
            4096.min(limit.saturating_sub(*retained).saturating_sub(bytes)),
        )
        .map_err(|_| ReferencePageLimit::Scan)?;
    let label_truncated = label.truncated;
    let label = label.text;
    let association = reference_form_associations(*occurrence, budget);
    let association = match association.state {
        ReferenceFormAssociationState::Unrecorded => ReferenceAssociation::Unrecorded {},
        ReferenceFormAssociationState::Complete => ReferenceAssociation::Valid {
            forms: association.forms,
            owner: semantic_owner
                .and_then(ContentRevealRef::to_owned)
                .ok_or(ReferencePageLimit::Position)?,
        },
        ReferenceFormAssociationState::Invalid => ReferenceAssociation::Invalid {},
        ReferenceFormAssociationState::Limited(_)
        | ReferenceFormAssociationState::MaterializationLimit => ReferenceAssociation::Limited {},
    };
    let total = bytes.saturating_add(label.len());
    if total > limit.saturating_sub(*retained) {
        return Err(ReferencePageLimit::MaterializationBytes);
    }
    *retained += total;
    Ok(ReferenceRecord {
        occurrence: occurrence.key,
        source_read: source_read(occurrence.location),
        origin,
        owner,
        label_preview: label,
        label_preview_truncated: label_truncated,
        association,
        resolution: resolution::initial(occurrence.target, source_address),
    })
}

fn attach_projection(
    document: &Document,
    result: &mut ReferenceInventory,
    record_bytes: &[usize],
    retained: &mut usize,
    limit: usize,
) -> Option<mant_ir::ContentKeyRemap> {
    if result.records.is_empty() {
        return None;
    }
    debug_assert_eq!(result.records.len(), record_bytes.len());
    let mut prefix_bytes = Vec::with_capacity(record_bytes.len() + 1);
    prefix_bytes.push(0usize);
    for &bytes in record_bytes {
        prefix_bytes.push(prefix_bytes.last().unwrap().saturating_add(bytes));
    }
    // Both the retained record bytes and the closed store grow with a source
    // prefix. Dense remapping cannot make an existing key shorter when a new
    // earlier key is inserted, so feasibility is monotone. At most ten
    // projections are needed for the maximum 1,000-record page.
    let mut admitted = 0usize;
    let mut rejected = result.records.len() + 1;
    let mut selected = None;
    while admitted + 1 < rejected {
        let candidate = admitted + (rejected - admitted) / 2;
        let mut builder = ContentProjectionBuilder::new(
            &document
                .flow()
                .expect("Flow reference inventory")
                .content_store,
        );
        let included = result
            .records
            .iter()
            .take(candidate)
            .try_for_each(|record| builder.include_link(record.occurrence));
        let projected =
            included
                .ok()
                .and_then(|()| builder.finish().ok())
                .and_then(|(projection, remap)| {
                    let available = limit.checked_sub(prefix_bytes[candidate])?;
                    let bytes = serialized_size(&projection, available)?;
                    Some((projection, remap, bytes))
                });
        if let Some(projection) = projected {
            admitted = candidate;
            selected = Some(projection);
        } else {
            rejected = candidate;
        }
    }
    if admitted < result.records.len() {
        result.records.truncate(admitted);
        result.page.limited = Some(ReferencePageLimit::MaterializationBytes);
    }
    let Some((projection, remap, projection_bytes)) = selected else {
        *retained = 0;
        return None;
    };
    *retained = prefix_bytes[admitted].saturating_add(projection_bytes);
    result.content_projection = Some(projection);
    Some(remap)
}

fn serialized_size(value: &impl serde::Serialize, limit: usize) -> Option<usize> {
    struct Count {
        remaining: usize,
    }
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.remaining = self
                .remaining
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("reference projection budget"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Count { remaining: limit };
    serde_json::to_writer(&mut writer, value).ok()?;
    Some(limit - writer.remaining)
}

fn source_read(location: mant_ir::ContentLocationRef<'_>) -> mant_protocol::ContentSelector {
    use std::fmt::Write;
    let sections = match location {
        mant_ir::ContentLocationRef::DocumentHeading { .. } => &[][..],
        mant_ir::ContentLocationRef::SectionHeading { sections, .. }
        | mant_ir::ContentLocationRef::Content { sections, .. } => sections,
    };
    let mut path = String::new();
    for index in sections {
        let number = u64::from(*index) + 1;
        let digits = number.ilog10() as usize + 1;
        // Deep positions can exceed the readable selector's 512-byte contract;
        // return the nearest representable ancestor, not an invalid selector.
        if path.len() + usize::from(!path.is_empty()) + digits > 512 {
            break;
        }
        if !path.is_empty() {
            path.push('.');
        }
        write!(&mut path, "{number}").expect("String write cannot fail");
    }
    mant_protocol::ContentSelector::path(if path.is_empty() {
        "root".to_owned()
    } else {
        path
    })
}

fn target_key(target: &LinkTarget) -> (ReferenceTargetType, &str, Option<&str>) {
    match target {
        LinkTarget::Document { name, fragment } => {
            (ReferenceTargetType::Document, name, fragment.as_deref())
        }
        LinkTarget::Manual {
            name,
            manual_section,
        } => (ReferenceTargetType::Manual, name, manual_section.as_deref()),
        LinkTarget::Section { id } => (ReferenceTargetType::Local, id.as_str(), None),
        LinkTarget::External { uri } => (ReferenceTargetType::External, uri, None),
        LinkTarget::Email { address } => (ReferenceTargetType::Email, address, None),
    }
}

fn target_size(target: &LinkTarget) -> usize {
    let (_, a, b) = target_key(target);
    a.len().saturating_add(b.map_or(0, str::len))
}
fn address_bytes(address: &DocumentAddress) -> usize {
    match address {
        DocumentAddress::Manual {
            name,
            manual_section,
        } => name.len().saturating_add(manual_section.len()),
        DocumentAddress::Markdown { path, origin } => path.len().saturating_add(match origin {
            mant_ir::MarkdownOrigin::Documents => 0,
            mant_ir::MarkdownOrigin::Source { name } => name.len(),
        }),
    }
}
