//! Operation-local destinations. Native cells carry short occurrence keys,
//! never another owned copy of an authored URI.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use mant_ir::{Block, Inline, LinkTarget, Section};

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
    target: LinkTarget,
}

mod projection;
#[cfg(test)]
use projection::target_bytes;
use projection::{
    coalesce, collect_nonempty, compact_styles, has_annotation, has_scalar,
    prune_provisional_identities,
};

const ENTRY: &str = "\0mant:annotation-entry:";

fn is_annotation_marker(id: &str) -> bool {
    [ENTRY, "\0mant:link-scope:", "\0mant:annotation-end:"]
        .iter()
        .any(|prefix| id.starts_with(prefix))
}

pub(in crate::mandoc) fn entry_marker(target: &LinkTarget) -> Option<String> {
    match target {
        LinkTarget::External { uri } if uri.starts_with(PREFIX) => Some(format!("{ENTRY}{uri}")),
        _ => None,
    }
}

const PREFIX: &str = "\0mant:annotation:";

#[derive(Clone, Debug, Default)]
pub(in crate::mandoc) struct AnnotationPool(Rc<RefCell<Destinations>>);

#[derive(Debug, Default)]
struct Destinations {
    deferred: bool,
    next: usize,
    targets: HashMap<String, Destination>,
    published: HashSet<String>,
    reduced: HashSet<String>,
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
                destinations.targets.remove(&format!("{PREFIX}{key}"));
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
            .map(|destination| read(&destination.target))
    }

    pub(in crate::mandoc) fn register(&self, target: LinkTarget) -> RegisteredAnnotation {
        // The allocator is operation-local and also shared by speculative
        // forks. A discarded fork can leave a vacant key, never reuse a
        // live source occurrence's identity. No key reaches public IR.
        let mut destinations = self.0.borrow_mut();
        if let LinkTarget::External { uri } = &target
            && let Some(destination) = destinations.targets.get(uri)
        {
            return RegisteredAnnotation {
                key: destination.key,
                target,
            };
        }
        let id =
            AnnotationKey(u32::try_from(destinations.next).expect("bounded source occurrences"));
        let key = format!("{PREFIX}{id}");
        destinations.next = destinations
            .next
            .checked_add(1)
            .expect("bounded authored annotations");
        #[cfg(test)]
        super::metrics::note(|counts| {
            counts.registered += 1;
            counts.registered_bytes += target_bytes(&target);
        });
        destinations
            .targets
            .insert(key.clone(), Destination { key: id, target });
        RegisteredAnnotation {
            key: id,
            target: LinkTarget::External { uri: key },
        }
    }

    pub(in crate::mandoc) fn finish_local(&self, nodes: &mut Vec<Inline>) {
        if !self.0.borrow().deferred {
            self.resolve(nodes);
        }
    }

    pub(in crate::mandoc) fn finish(&self, blocks: &mut [Block], sections: &mut [Section]) {
        if self.0.borrow().targets.is_empty() {
            return;
        }
        self.blocks(blocks);
        for section in sections {
            self.resolve(&mut section.heading.content);
            self.blocks(&mut section.blocks);
            self.finish(&mut [], &mut section.children);
        }
    }

    fn blocks(&self, blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                    self.resolve(children);
                }
                Block::List { items, .. } => {
                    for item in items {
                        self.blocks(&mut item.blocks);
                    }
                }
                Block::DefinitionList { items, .. } => {
                    for item in items {
                        for term in &mut item.terms {
                            self.resolve(&mut term.content);
                        }
                        self.blocks(&mut item.description);
                    }
                }
                Block::Table { rows, .. } => {
                    for row in rows {
                        for cell in &mut row.cells {
                            self.blocks(&mut cell.blocks);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn resolve(&self, nodes: &mut Vec<Inline>) {
        if !has_annotation(nodes) {
            return;
        }
        let mut nonempty = HashSet::new();
        collect_nonempty(nodes, &mut nonempty);
        prune_provisional_identities(nodes, &nonempty);
        coalesce(nodes);
        self.publish(nodes, false, &nonempty);
    }

    pub(in crate::mandoc) fn take_diagnostics(&self) -> Vec<mant_ir::Diagnostic> {
        self.0.borrow_mut().reduced.drain().map(|_| mant_ir::Diagnostic {
            level: mant_ir::DiagnosticLevel::Warning,
            impact: mant_ir::DiagnosticImpact::SemanticCoverage,
            code: Some("manual.annotation-coverage".into()),
            message: "a source annotation crossed an independent reference or content root; its initial identity and all accepted text were retained".into(),
            source: None,
        }).collect()
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
            if matches!(&node, Inline::Anchor { id, .. } if is_annotation_marker(id.as_str())) {
                continue;
            }
            if let Inline::Link {
                target: LinkTarget::External { uri },
                children,
                ..
            } = &mut node
                && self.0.borrow().targets.contains_key(uri)
            {
                // An empty provisional receipt must not precede its accepted
                // label as another public occurrence in this same root.
                let has_content = has_scalar(children);
                if !has_content && nonempty.contains(uri) {
                    output.append(children);
                    continue;
                }
                let foreign = children.iter().position(|child| matches!(child, Inline::Link { target: LinkTarget::External { uri: other }, .. } if other.starts_with(PREFIX) && other != uri));
                let cut = foreign.map(|at| {
                    let Inline::Link { target, .. } = &children[at] else { unreachable!() };
                    let marker = entry_marker(target);
                    marker.and_then(|marker| children[..at].iter().rposition(|node| matches!(node, Inline::Anchor { id, .. } if id.as_str() == marker))).unwrap_or(at)
                });
                let mut tail = cut.map_or_else(Vec::new, |at| children.split_off(at));
                self.publish(children, true, nonempty);
                let mut destinations = self.0.borrow_mut();
                if foreign.is_some() {
                    destinations.reduced.insert(uri.clone());
                }
                if destinations.published.insert(uri.clone()) {
                    let target = destinations
                        .targets
                        .get(uri)
                        .expect("live annotation")
                        .target
                        .clone();
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
                        destinations.reduced.insert(uri.clone());
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
}
