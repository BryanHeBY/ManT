//! Snapshot-local, bounded verification of canonical anchors and authored aliases.
use std::{collections::HashMap, ops::ControlFlow, sync::Arc};

use mant_ir::{
    ContentReveal, ContentRevealRef, Document, NavigationEvent, NavigationScanOptions,
    ReferenceLinkFilter, ReferenceScanLimits, ReferenceScope, scan_navigation_scope,
};

const MAX_TARGETS: usize = 32_768;
const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

#[derive(Debug)]
enum Target {
    Unique(Arc<ContentReveal>),
    Ambiguous,
}

/// Never cache a first match as verified until the entire bounded scan completes.
#[derive(Debug)]
pub(super) struct TargetIndex {
    targets: HashMap<String, Target>,
    complete: bool,
}

impl TargetIndex {
    pub(super) fn build(document: Option<&Document>) -> Self {
        let Some(document) = document else {
            return Self {
                targets: HashMap::new(),
                complete: true,
            };
        };
        Self::build_with_limits(
            document,
            ReferenceScanLimits {
                steps: mant_ir::MAX_REFERENCE_SCAN_STEPS,
                bytes: mant_ir::MAX_REFERENCE_SCAN_BYTES,
                ..ReferenceScanLimits::default()
            },
            MAX_TARGETS,
            MAX_PAYLOAD,
        )
    }

    fn build_with_limits(
        document: &Document,
        limits: ReferenceScanLimits,
        max_targets: usize,
        max_payload: usize,
    ) -> Self {
        let mut result = Self {
            targets: HashMap::new(),
            complete: false,
        };
        let mut payload = 0usize;
        let report = scan_navigation_scope(
            document,
            ReferenceScope::Document,
            limits,
            NavigationScanOptions {
                links: ReferenceLinkFilter::NONE,
                targets: true,
                entry_sets: false,
            },
            |event, budget| {
                let NavigationEvent::Target(target) = event else {
                    return ControlFlow::Continue(());
                };
                let mut owned = None;
                for key in std::iter::once(target.id.as_str())
                    .chain(target.aliases.iter().map(mant_ir::FragmentAlias::as_str))
                {
                    // Charge hashing and coordinate comparison even for repeated aliases.
                    if budget
                        .consume(
                            target.reveal.depth(),
                            target.reveal.depth() * 3 + 1,
                            key.len(),
                        )
                        .is_err()
                    {
                        return ControlFlow::Break(());
                    }
                    if let Some(previous) = result.targets.get_mut(key) {
                        if let Target::Unique(retained) = previous
                            && retained.as_ref().as_ref() != target.reveal
                        {
                            *previous = Target::Ambiguous;
                        }
                        continue;
                    }
                    let bytes = retained_bytes(key, target.reveal);
                    if result.targets.len() >= max_targets
                        || bytes > max_payload.saturating_sub(payload)
                        || budget.consume(target.reveal.depth(), 1, bytes).is_err()
                    {
                        return ControlFlow::Break(());
                    }
                    if owned.is_none() {
                        let Some(reveal) = target.reveal.to_owned() else {
                            return ControlFlow::Break(());
                        };
                        owned = Some(Arc::new(reveal));
                    }
                    payload += bytes;
                    result.targets.insert(
                        key.to_owned(),
                        Target::Unique(Arc::clone(owned.as_ref().expect("retained destination"))),
                    );
                }
                ControlFlow::Continue(())
            },
        );
        result.complete = report.complete();
        result
    }

    pub(super) fn validate(&self, fragment: &str, has_tldr: bool) -> Result<(), String> {
        let target = self.targets.get(fragment);
        let tldr = has_tldr && fragment == super::TLDR_ID;
        if matches!(target, Some(Target::Ambiguous)) || (tldr && target.is_some()) {
            Err(format!("Ambiguous local target #{fragment}"))
        } else if !self.complete {
            Err(format!(
                "Local target #{fragment} was not verified within the navigation budget"
            ))
        } else if target.is_none() && !tldr {
            Err(format!("No outline node matches #{fragment}"))
        } else {
            Ok(())
        }
    }
}

fn retained_bytes(key: &str, reveal: ContentRevealRef<'_>) -> usize {
    // Conservatively charge each alias for its own destination, even though Arc
    // shares it. Include map/allocator overhead and actual coordinate storage,
    // not only the smaller encoded representation.
    key.len()
        + 256
        + std::mem::size_of::<ContentReveal>()
        + reveal.depth() * std::mem::size_of::<mant_ir::ContentBlockStep>()
        + reveal.encoded_size_bound()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> Document {
        mant_loader::load_markdown_text("## Description\n\nBody.\n", None)
            .unwrap()
            .document
            .unwrap()
    }

    #[test]
    fn aliases_merge_only_for_the_same_structural_destination() {
        let mut document = document();
        document.sections[0].fragment_aliases = vec!["shared".into(), "shared".into()];
        let index = TargetIndex::build(Some(&document));
        assert_eq!(index.validate("shared", false), Ok(()));
        assert_eq!(index.validate("description", false), Ok(()));
        assert!(
            index
                .validate("missing", false)
                .unwrap_err()
                .contains("No outline")
        );
        let mut other = document.sections[0].clone();
        other.id = "other".into();
        document.sections.push(other);
        let index = TargetIndex::build(Some(&document));
        assert!(
            index
                .validate("shared", false)
                .unwrap_err()
                .contains("Ambiguous")
        );
        assert_eq!(index.validate("description", false), Ok(()));
    }

    #[test]
    fn incomplete_indexes_never_verify_found_missing_or_synthetic_targets() {
        let mut document = document();
        let mut second = document.sections[0].clone();
        second.id = "second".into();
        document.sections.push(second);
        let defaults = ReferenceScanLimits::default();
        for (limits, records, payload) in [
            (
                ReferenceScanLimits {
                    steps: 1,
                    ..defaults
                },
                MAX_TARGETS,
                MAX_PAYLOAD,
            ),
            (
                ReferenceScanLimits {
                    bytes: 1,
                    ..defaults
                },
                MAX_TARGETS,
                MAX_PAYLOAD,
            ),
            (
                ReferenceScanLimits {
                    depth: 0,
                    ..defaults
                },
                MAX_TARGETS,
                MAX_PAYLOAD,
            ),
            (defaults, 1, MAX_PAYLOAD),
            (defaults, MAX_TARGETS, 1),
        ] {
            let index = TargetIndex::build_with_limits(&document, limits, records, payload);
            assert!(!index.complete);
            for (fragment, tldr) in [("description", false), ("missing", false), ("tldr", true)] {
                assert!(
                    index
                        .validate(fragment, tldr)
                        .unwrap_err()
                        .contains("navigation budget")
                );
            }
        }
    }

    #[test]
    fn quick_reference_only_views_still_validate_the_synthetic_target() {
        let index = TargetIndex::build(None);
        assert_eq!(index.validate("tldr", true), Ok(()));
        assert!(
            index
                .validate("tldr", false)
                .unwrap_err()
                .contains("No outline")
        );
    }

    #[test]
    fn large_alias_inventory_charges_retention_without_reusing_the_small_scan_budget() {
        let mut document = document();
        let mut section = document.sections.remove(0);
        section.blocks.clear();
        document.sections = (0..13_000)
            .map(|index| {
                let mut section = section.clone();
                section.id = format!("section-{index}").into();
                section.fragment_aliases = vec![format!("alias-{index}").into()];
                section
            })
            .collect();
        let index = TargetIndex::build(Some(&document));
        assert_eq!(index.targets.len(), 26_000);
        assert_eq!(index.validate("section-0", false), Ok(()));
        assert_eq!(index.validate("alias-12999", false), Ok(()));
        assert!(index.complete);
    }
}
