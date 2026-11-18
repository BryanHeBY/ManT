//! Section-reference proof from authored titles, heading owners, and IR outcomes.

use std::collections::{BTreeMap, HashMap, VecDeque};

use libmandoc_rs::{Node, NodeKind};
use mant_ir::{Diagnostic, Section};

use super::{node_part_children, observe_operand};

type Origin = (u32, u32);

#[derive(Default)]
pub(crate) struct NativeSectionLinks {
    headings: Vec<(Origin, String)>,
    references: Vec<(Origin, String)>,
}

#[derive(Default)]
pub(crate) struct ObservedSectionLinks {
    headings: BTreeMap<Origin, Vec<String>>,
    resolved: Vec<String>,
    unresolved: BTreeMap<String, usize>,
}

impl NativeSectionLinks {
    pub(crate) fn record_heading(&mut self, node: &Node, default_name: Option<&str>) {
        self.headings.push((
            (node.line, node.column),
            mant_codec::observe_roff_section_phrase(
                node_part_children(node, NodeKind::Head),
                default_name,
            ),
        ));
    }

    pub(crate) fn record_reference(&mut self, node: &Node, default_name: Option<&str>) -> bool {
        let title = mant_codec::observe_roff_section_phrase(&node.children, default_name);
        if title.is_empty()
            && node.children.iter().all(|child| {
                child
                    .text
                    .as_deref()
                    .and_then(observe_operand)
                    .is_some_and(|value| value.is_empty())
            })
        {
            // Sx may have no operand or display glyph at all. Space escapes
            // are different: they still execute a visible, unresolved label.
            return false;
        }
        self.references.push(((node.line, node.column), title));
        true
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
            if let Some(id) = mant_codec::observe_roff_section_target(&targets, title) {
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

    fn authored_targets(
        &self,
        observed: &ObservedSectionLinks,
    ) -> Option<HashMap<String, Option<String>>> {
        let mut by_origin: BTreeMap<_, VecDeque<_>> = observed
            .headings
            .iter()
            .map(|(origin, ids)| (*origin, ids.iter().collect()))
            .collect();
        let mut targets = HashMap::new();
        for (origin, title) in &self.headings {
            let id = by_origin.get_mut(origin)?.pop_front()?;
            targets
                .entry(title.clone())
                .and_modify(|target| *target = None)
                .or_insert_with(|| Some(id.clone()));
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
