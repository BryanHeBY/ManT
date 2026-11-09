//! Operation-local destinations. Native cells carry short occurrence keys,
//! never another owned copy of an authored URI.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use mant_ir::{Block, EquationExpression, Inline, LinkTarget, Section, SourceSpan};

use super::markers;

mod candidates;
use candidates::{finish_source_candidates, protect_source_label_candidates};
mod equations;
use equations::EquationDestination;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::mandoc) struct AnnotationKey(u32);
impl std::fmt::Display for AnnotationKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}
impl std::str::FromStr for AnnotationKey {
    type Err = std::num::ParseIntError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}
#[cfg(test)]
impl From<u32> for AnnotationKey {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

pub(in crate::mandoc) struct RegisteredAnnotation {
    pub key: AnnotationKey,
    pub target: LinkTarget,
}
#[derive(Debug)]
struct Destination {
    key: AnnotationKey,
    payload: DestinationPayload,
}

#[derive(Debug)]
enum DestinationPayload {
    Link(LinkTarget),
    Equation(Box<EquationDestination>),
}

mod projection;
#[cfg(test)]
use projection::target_bytes;
use projection::{
    coalesce, collect_nonempty, compact_styles, has_annotation, has_scalar,
    prune_provisional_identities,
};

#[derive(Clone, Debug, Default)]
pub(in crate::mandoc) struct AnnotationPool(Rc<RefCell<Destinations>>);

#[derive(Debug, Default)]
struct Destinations {
    deferred: bool,
    next: usize,
    source_candidates: usize,
    targets: HashMap<String, Destination>,
    published: HashSet<String>,
    reduced: HashMap<AnnotationKey, Option<SourceSpan>>,
}

pub(in crate::mandoc) struct AnnotationTransaction {
    pool: AnnotationPool,
    first: usize,
    committed: bool,
}

impl AnnotationTransaction {
    pub(in crate::mandoc) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for AnnotationTransaction {
    fn drop(&mut self) {
        if !self.committed {
            let mut destinations = self.pool.0.borrow_mut();
            for key in self.first..destinations.next {
                destinations.targets.remove(&markers::target_key(key));
            }
        }
    }
}

impl PartialEq for AnnotationPool {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for AnnotationPool {}

impl AnnotationPool {
    pub(in crate::mandoc) fn transaction(&self) -> AnnotationTransaction {
        AnnotationTransaction {
            pool: self.clone(),
            first: self.0.borrow().next,
            committed: false,
        }
    }
    pub(in crate::mandoc) fn deferred() -> Self {
        let pool = Self::default();
        pool.0.borrow_mut().deferred = true;
        pool
    }

    pub(in crate::mandoc) fn with_destination<R>(
        &self,
        key: &str,
        read: impl FnOnce(&LinkTarget) -> R,
    ) -> Option<R> {
        self.0
            .borrow()
            .targets
            .get(key)
            .and_then(|destination| match &destination.payload {
                DestinationPayload::Link(target) => Some(read(target)),
                DestinationPayload::Equation(_) => None,
            })
    }

    pub(in crate::mandoc) fn register(&self, target: LinkTarget) -> RegisteredAnnotation {
        // The allocator is operation-local and also shared by speculative
        // forks. A discarded fork can leave a vacant key, never reuse a
        // live source occurrence's identity. No key reaches public IR.
        let destinations = self.0.borrow();
        if let LinkTarget::External { uri } = &target
            && let Some(destination) = destinations.targets.get(uri)
        {
            return RegisteredAnnotation {
                key: destination.key,
                target,
            };
        }
        drop(destinations);
        #[cfg(test)]
        super::metrics::note(|counts| {
            counts.registered += 1;
            counts.registered_bytes += target_bytes(&target);
        });
        self.insert_destination(DestinationPayload::Link(target))
    }

    pub(in crate::mandoc) fn register_equation(
        &self,
        expression: EquationExpression,
        source: Option<SourceSpan>,
    ) -> RegisteredAnnotation {
        self.insert_destination(DestinationPayload::Equation(Box::new(
            EquationDestination::new(expression, source),
        )))
    }

    pub(in crate::mandoc) fn record_equation_projection(
        &self,
        key: AnnotationKey,
        children: &[Inline],
        complete: bool,
    ) {
        let mut destinations = self.0.borrow_mut();
        if let Some(Destination {
            payload: DestinationPayload::Equation(equation),
            ..
        }) = destinations.targets.get_mut(&markers::target_key(key))
        {
            equation.record(children, complete);
        }
    }

    pub(in crate::mandoc) fn mark_equation_incomplete(&self, key: AnnotationKey) {
        let mut destinations = self.0.borrow_mut();
        if let Some(Destination {
            payload: DestinationPayload::Equation(equation),
            ..
        }) = destinations.targets.get_mut(&markers::target_key(key))
        {
            equation.mark_incomplete();
        }
    }

    pub(in crate::mandoc) fn record_equation_deferred(
        &self,
        key: AnnotationKey,
        children: &[Inline],
    ) {
        let mut destinations = self.0.borrow_mut();
        if let Some(Destination {
            payload: DestinationPayload::Equation(equation),
            ..
        }) = destinations.targets.get_mut(&markers::target_key(key))
        {
            equation.record_deferred(children);
        }
    }

    fn insert_destination(&self, payload: DestinationPayload) -> RegisteredAnnotation {
        // Only this occurrence owns the equation tree or destination. Native
        // word splits and deferred glyphs carry the short private key.
        let mut destinations = self.0.borrow_mut();
        let id =
            AnnotationKey(u32::try_from(destinations.next).expect("bounded source occurrences"));
        let key = markers::target_key(id);
        destinations.next = destinations
            .next
            .checked_add(1)
            .expect("bounded authored annotations");
        destinations
            .targets
            .insert(key.clone(), Destination { key: id, payload });
        RegisteredAnnotation {
            key: id,
            target: LinkTarget::External { uri: key },
        }
    }

    pub(in crate::mandoc) fn finish_local(&self, nodes: &mut Vec<Inline>) {
        let destinations = self.0.borrow();
        let deferred = destinations.deferred;
        let has_source_candidates = destinations.source_candidates > 0;
        drop(destinations);
        if !deferred {
            self.check_local_equation_carriers(nodes);
            self.finish_inline_root(nodes, has_source_candidates);
        }
    }

    pub(in crate::mandoc) fn register_source_candidate(&self) -> LinkTarget {
        let mut destinations = self.0.borrow_mut();
        let id = destinations.source_candidates;
        destinations.source_candidates += 1;
        markers::candidate_target(id)
    }

    pub(in crate::mandoc) fn note_unrepresentable_member(
        &self,
        target: &LinkTarget,
        source: Option<SourceSpan>,
    ) {
        let LinkTarget::External { uri } = target else {
            return;
        };
        let mut destinations = self.0.borrow_mut();
        let Some(destination) = destinations.targets.get_mut(uri) else {
            return;
        };
        if let DestinationPayload::Equation(equation) = &mut destination.payload {
            equation.mark_incomplete();
            return;
        }
        let key = destination.key;
        let first = destinations.reduced.entry(key).or_insert(None);
        *first = first.or(source);
    }

    pub(in crate::mandoc) fn finish(
        &self,
        blocks: &mut [Block],
        sections: &mut [Section],
        evidence: &mut crate::definitions::NativeHeadEvidence,
    ) {
        let has_source_candidates = self.0.borrow().source_candidates > 0;
        if self.0.borrow().targets.is_empty() && !has_source_candidates {
            return;
        }
        self.check_document_equation_carriers(blocks, sections);
        evidence.publish_annotations(blocks, sections, |nodes| {
            self.finish_inline_root(nodes, has_source_candidates)
        });
    }

    fn finish_inline_root(&self, nodes: &mut Vec<Inline>, has_source_candidates: bool) -> bool {
        let changed = self.resolve(nodes, has_source_candidates);
        if has_source_candidates {
            finish_source_candidates(nodes) || changed
        } else {
            changed
        }
    }

    fn resolve(&self, nodes: &mut Vec<Inline>, has_source_candidates: bool) -> bool {
        if !has_annotation(nodes) {
            return false;
        }
        let mut nonempty = HashSet::new();
        collect_nonempty(nodes, &mut nonempty);
        prune_provisional_identities(nodes, &nonempty);
        coalesce(nodes);
        if has_source_candidates {
            protect_source_label_candidates(nodes);
        }
        self.publish(nodes, false, &nonempty);
        true
    }

    pub(in crate::mandoc) fn take_diagnostics(&self) -> Vec<mant_ir::Diagnostic> {
        let mut reduced: Vec<_> = self.0.borrow_mut().reduced.drain().collect();
        reduced.sort_unstable_by_key(|(key, _)| key.0);
        let mut diagnostics: Vec<_> = reduced.into_iter().map(|(_, source)| mant_ir::Diagnostic {
            level: mant_ir::DiagnosticLevel::Warning,
            impact: mant_ir::DiagnosticImpact::SemanticCoverage,
            code: Some("manual.annotation-coverage".into()),
            message: "a source annotation could not include all its accepted members in one owner; its initial identity and all accepted content were retained".into(),
            source,
        }).collect();
        self.take_equation_diagnostics(&mut diagnostics);
        diagnostics
    }

    fn publish(&self, nodes: &mut Vec<Inline>, compact: bool, nonempty: &HashSet<String>) {
        if compact {
            compact_styles(nodes);
        }
        let mut output = Vec::with_capacity(nodes.len());
        for mut node in std::mem::take(nodes) {
            #[cfg(test)]
            super::metrics::note(|counts| counts.normalization += 1);
            // Entry receipts serve foreign-source cuts only. Consume them
            // here, after the enclosing cut was located, before public anchor
            // normalization can give them a document navigation identity.
            if matches!(&node, Inline::Anchor { id, .. } if markers::is_annotation_marker(id.as_str()))
            {
                continue;
            }
            if let Inline::Link {
                target: LinkTarget::External { uri },
                children,
                ..
            } = &mut node
                && self.0.borrow().targets.contains_key(uri)
            {
                if self.publish_equation(uri, children, &mut output, nonempty) {
                    continue;
                }
                // An empty provisional receipt must not precede its accepted
                // label as another public occurrence in this same root.
                let has_content = has_scalar(children);
                if !has_content && nonempty.contains(uri) {
                    output.append(children);
                    continue;
                }
                let foreign = children.iter().position(|child| matches!(child, Inline::Link { target: LinkTarget::External { uri: other }, .. } if other != uri && self.is_link_destination(other)));
                let cut = foreign.map(|at| {
                    let Inline::Link { target, .. } = &children[at] else { unreachable!() };
                    let marker = markers::entry_marker(target);
                    marker.and_then(|marker| children[..at].iter().rposition(|node| matches!(node, Inline::Anchor { id, .. } if id.as_str() == marker))).unwrap_or(at)
                });
                let mut tail = cut.map_or_else(Vec::new, |at| children.split_off(at));
                self.publish(children, true, nonempty);
                let mut destinations = self.0.borrow_mut();
                let key = destinations.targets.get(uri).expect("live annotation").key;
                if foreign.is_some() {
                    destinations.reduced.entry(key).or_insert(None);
                }
                if destinations.published.insert(uri.clone()) {
                    let DestinationPayload::Link(target) = &destinations
                        .targets
                        .get(uri)
                        .expect("live annotation")
                        .payload
                    else {
                        unreachable!("equations publish separately")
                    };
                    let target = target.clone();
                    #[cfg(test)]
                    super::metrics::note(|counts| {
                        counts.published += 1;
                        counts.published_bytes += target_bytes(&target);
                    });
                    output.push(Inline::Link {
                        target,
                        title: None,
                        children: std::mem::take(children),
                    });
                } else {
                    if has_content {
                        destinations.reduced.entry(key).or_insert(None);
                    }
                    output.append(children);
                }
                drop(destinations);
                self.publish(&mut tail, false, nonempty);
                output.append(&mut tail);
                continue;
            }
            match &mut node {
                Inline::Link { children, .. }
                | Inline::Strong { children }
                | Inline::Emphasis { children } => self.publish(children, compact, nonempty),
                _ => {}
            }
            output.push(node);
        }
        *nodes = output;
    }

    fn is_link_destination(&self, uri: &str) -> bool {
        self.0
            .borrow()
            .targets
            .get(uri)
            .is_some_and(|destination| matches!(destination.payload, DestinationPayload::Link(_)))
    }
}
