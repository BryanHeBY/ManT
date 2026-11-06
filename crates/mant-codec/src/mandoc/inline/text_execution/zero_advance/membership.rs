//! Born membership and actual root qualification, independent of native words.
use super::{Arc, Decoration, ZeroAdvanceState};
use crate::mandoc::inline::ownership::{Annotation, OutputMembership};

impl ZeroAdvanceState {
    pub(in crate::mandoc) fn has_annotation_scope(&self) -> bool {
        !self.output_owners.is_empty()
    }

    pub(in crate::mandoc) fn new_annotation(
        &mut self,
        target: mant_ir::LinkTarget,
    ) -> crate::mandoc::inline::ownership::annotations::RegisteredAnnotation {
        self.annotations
            .get_or_insert_with(Default::default)
            .register(target)
    }
    pub(in crate::mandoc) fn annotation_target(
        &mut self,
        target: mant_ir::LinkTarget,
    ) -> mant_ir::LinkTarget {
        self.new_annotation(target).target
    }
    pub(in crate::mandoc) fn qualify_current_link(&mut self, target: mant_ir::LinkTarget) {
        let epoch = self.output_owners.last().expect("active man annotation").0;
        self.active_links.push((epoch, target));
    }

    pub(in crate::mandoc) fn has_qualified_root(&self) -> bool {
        self.output_owners.iter().any(|scope| {
            scope.1 == self.semantic_root
                && self.active_links.iter().any(|(owner, _)| *owner == scope.0)
        })
    }

    pub(in crate::mandoc) fn close_annotation_root(&mut self) -> Vec<String> {
        let scopes: Vec<_> = self
            .output_owners
            .iter()
            .filter(|scope| {
                scope.1 == self.semantic_root
                    && self.active_links.iter().any(|(owner, _)| *owner == scope.0)
            })
            .copied()
            .collect();
        // Normal recursive return binds the innermost source first. A real
        // root close must build the same decoration order for still-pending
        // cells, so an outer partial annotation cannot erase the inner label.
        for &(epoch, root) in scopes.iter().rev() {
            if let Some((_, target)) = self.active_links.iter().find(|(owner, _)| *owner == epoch) {
                let annotation = Annotation::Link {
                    target: target.clone(),
                    title: None,
                };
                // Bind before the native close flushes a born glyph. The
                // marker closes new admission, not that glyph's ownership.
                self.bind_deferred(
                    OutputMembership {
                        epoch,
                        root,
                        enclosing: None,
                    },
                    &annotation,
                    true,
                );
            }
        }
        self.semantic_root = self
            .semantic_root
            .checked_add(1)
            .expect("bounded annotation roots");
        scopes
            .into_iter()
            .map(|scope| format!("\0mant:annotation-end:{}", scope.0))
            .collect()
    }
    /// A semantic wrapper can begin while BACKBEFORE still owns a glyph from
    /// preceding source. Track that glyph through the shared formatter until
    /// it is emitted or overstruck, including across nested wrappers.
    pub(in crate::mandoc) fn begin_output_owner(&mut self) {
        self.output_epoch = self
            .output_epoch
            .checked_add(1)
            .expect("bounded annotation entries");
        self.output_owners
            .push((self.output_epoch, self.semantic_root));
    }

    pub(in crate::mandoc) fn end_output_owner(&mut self) -> OutputMembership {
        let (epoch, root) = self.output_owners.pop().expect("balanced annotation owner");
        self.active_links.retain(|(owner, _)| *owner != epoch);
        OutputMembership {
            epoch,
            root,
            enclosing: self.output_owners.last().map(|scope| scope.0),
        }
    }

    /// Annotate this scope's members which remain native input. Birth epochs
    /// are monotone even after returning from a nested scope, so old deferred
    /// history is skipped in logarithmic time rather than rescanned per word.
    pub(in crate::mandoc) fn bind_deferred(
        &mut self,
        scope: OutputMembership,
        annotation: &Annotation,
        continuation: bool,
    ) {
        let mut decorations = std::collections::HashMap::new();
        if let Some(glyph) = self
            .machine
            .pending_mut()
            .filter(|glyph| glyph.epoch >= scope.epoch && glyph.root == scope.root)
        {
            #[cfg(test)]
            crate::mandoc::inline::ownership::metrics::note(|counts| counts.bound += 1);
            glyph.decoration = Some(attach(
                &mut decorations,
                annotation,
                glyph.decoration.take(),
            ));
            glyph.link_continuation |= continuation;
        }
        let items = self.machine.deferred_projection_mut();
        let first = items.partition_point(|glyph| glyph.epoch < scope.epoch);
        for glyph in items[first..]
            .iter_mut()
            .filter(|glyph| glyph.root == scope.root)
        {
            #[cfg(test)]
            crate::mandoc::inline::ownership::metrics::note(|counts| counts.bound += 1);
            glyph.decoration = Some(attach(
                &mut decorations,
                annotation,
                glyph.decoration.take(),
            ));
            glyph.link_continuation |= continuation;
        }
        let first = self.held.partition_point(|cell| cell.epoch < scope.epoch);
        for cell in self.held[first..]
            .iter_mut()
            .filter(|cell| cell.root == scope.root)
        {
            cell.decoration = Some(attach(&mut decorations, annotation, cell.decoration.take()));
            cell.link_continuation |= continuation;
        }
    }
}

fn attach(
    cache: &mut std::collections::HashMap<usize, Arc<Decoration>>,
    annotation: &Annotation,
    previous: Option<Arc<Decoration>>,
) -> Arc<Decoration> {
    let key = previous
        .as_ref()
        .map_or(0, |value| Arc::as_ptr(value) as usize);
    Arc::clone(cache.entry(key).or_insert_with(|| {
        let mut layer = previous.as_deref();
        while let Some(decoration) = layer {
            if decoration.annotation == *annotation {
                return previous.expect("existing annotation layer");
            }
            layer = decoration.previous.as_deref();
        }
        #[cfg(test)]
        crate::mandoc::inline::ownership::metrics::note(|counts| counts.decorations += 1);
        Arc::new(Decoration {
            annotation: annotation.clone(),
            previous,
        })
    }))
}
