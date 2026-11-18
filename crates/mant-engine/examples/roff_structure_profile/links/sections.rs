//! Section-reference proof from authored titles, heading owners, and IR outcomes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use libmandoc_rs::Node;
use mant_codec::{RoffSectionHeading, RoffSectionTargets, SectionLabelEvidence};
use mant_ir::{Diagnostic, Section};

type Origin = (u32, u32);

#[derive(Default)]
pub(crate) struct NativeSectionLinks {
    headings: Vec<(Origin, RoffSectionHeading)>,
    native_targets: RoffSectionTargets,
    references: Vec<(Origin, Option<String>)>,
    uncertain_labels: Vec<Origin>,
}

#[derive(Default)]
pub(crate) struct ObservedSectionLinks {
    headings: BTreeMap<Origin, Vec<String>>,
    resolved: Vec<String>,
    unresolved: BTreeMap<String, usize>,
}

impl NativeSectionLinks {
    pub(crate) fn record_owner(&mut self, node: &Node) {
        self.native_targets.record_non_heading_owner(node);
    }

    pub(crate) fn record_heading(&mut self, node: &Node) {
        self.headings.push((
            (node.line, node.column),
            RoffSectionHeading::from_node(node),
        ));
    }

    pub(crate) fn record_reference(&mut self, node: &Node, errors: &BTreeSet<Origin>) -> bool {
        let origin = (node.line, node.column);
        let evidence = mant_codec::observe_roff_section_label_evidence(&node.children);
        let uncertain = evidence == SectionLabelEvidence::RecoveryOnly
            && node.children.iter().any(|child| {
                let start = (child.line, child.column);
                let width = child.text.as_ref().map_or(0, String::len);
                let end = start
                    .1
                    .saturating_add(u32::try_from(width).unwrap_or(u32::MAX));
                errors.range(start..(child.line, end)).next().is_some()
            });
        if uncertain {
            self.uncertain_labels.push(origin);
        }
        let has_label = !uncertain && evidence != SectionLabelEvidence::Invisible;
        let title = has_label.then(|| mant_codec::observe_roff_section_phrase(&node.children));
        // A control-only Sx still has a source occurrence. It owes no
        // link or diagnostic because there is no label to activate.
        self.references.push((origin, title));
        has_label
    }

    pub(crate) fn uncertain_labels(&self) -> &[Origin] {
        &self.uncertain_labels
    }

    #[cfg(test)]
    pub(crate) fn source_reference_counts(&self) -> (usize, usize) {
        let glyphless = self
            .references
            .iter()
            .filter(|(_, title)| title.is_none())
            .count();
        (
            self.references.len(),
            glyphless - self.uncertain_labels.len(),
        )
    }

    pub(crate) fn compare(&self, observed: &ObservedSectionLinks, violations: &mut Vec<String>) {
        if self.references.is_empty() {
            return;
        }
        let Some(targets) = self.authored_targets(observed) else {
            // Missing provenance leaves only the count check. Use the IR
            // heading owners to identify canonical IDs, never the links.
            return;
        };
        let mut position = 0;
        let mut unresolved = observed.unresolved.clone();
        for ((line, column), title) in &self.references {
            let Some(title) = title else { continue };
            if let Some(id) = targets.resolve(title) {
                // A label can acquire more than one annotation while crossing
                // an IR output segment. Preserve order for all authored links,
                // while allowing additional observations of one annotation.
                let remaining = &observed.resolved[position..];
                if let Some(offset) = remaining.iter().position(|candidate| candidate == id) {
                    position += offset + 1;
                } else {
                    violations.push(format!(
                        "section-link at {line}:{column}: expected {title:?} to resolve to {id:?} in source order"
                    ));
                }
            } else if let Some(count) = unresolved.get_mut(title)
                && *count > 0
            {
                *count -= 1;
            } else {
                violations.push(format!(
                    "section-link at {line}:{column}: expected unresolved diagnostic for {title:?}"
                ));
            }
        }
    }

    fn authored_targets(&self, observed: &ObservedSectionLinks) -> Option<RoffSectionTargets> {
        let mut by_origin: BTreeMap<_, VecDeque<_>> = observed
            .headings
            .iter()
            .map(|(origin, ids)| (*origin, ids.iter().collect()))
            .collect();
        let mut targets = self.native_targets.clone();
        for (origin, heading) in &self.headings {
            let id = by_origin.get_mut(origin)?.pop_front()?;
            targets.register(heading.clone(), id.clone());
        }
        by_origin
            .values()
            .all(VecDeque::is_empty)
            .then_some(targets)
    }
}

impl ObservedSectionLinks {
    pub(crate) fn record_heading(&mut self, section: &Section) {
        if let Some(source) = section.source {
            self.headings
                .entry((source.line, source.column))
                .or_default()
                .push(section.id.to_string());
        }
    }

    pub(crate) fn record_resolved(&mut self, id: &str) {
        self.resolved.push(id.to_owned());
    }

    pub(crate) fn record_diagnostic(&mut self, diagnostic: &Diagnostic) {
        if diagnostic.code.as_deref() == Some("unresolved-section-reference")
            && let Some(title) = diagnostic
                .message
                .strip_prefix("cannot resolve section reference: ")
        {
            *self.unresolved.entry(title.to_owned()).or_default() += 1;
        }
    }
}
