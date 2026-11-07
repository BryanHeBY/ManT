//! Born membership and actual root qualification, independent of native words.
use super::{Arc, Decoration, ZeroAdvanceState};
use crate::mandoc::inline::ownership::{Annotation, OutputMembership, markers};

/// The live annotation roots and their monotone member identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MembershipState {
    pub(super) output_owners: Vec<(u64, u64)>,
    pub(super) semantic_root: u64,
    pub(super) active_links: Vec<(u64, mant_ir::LinkTarget)>,
    pub(super) output_epoch: u64,
    pub(super) authored_member_epoch: u64,
}

impl MembershipState {
    pub(super) const fn new() -> Self {
        Self {
            output_owners: Vec::new(),
            output_epoch: 0,
            authored_member_epoch: 0,
            semantic_root: 0,
            active_links: Vec::new(),
        }
    }
}

impl ZeroAdvanceState {
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
        let epoch = self
            .membership
            .output_owners
            .last()
            .expect("active man annotation")
            .0;
        self.membership.active_links.push((epoch, target));
    }

    pub(in crate::mandoc) fn has_qualified_root(&self) -> bool {
        self.membership.output_owners.iter().any(|scope| {
            scope.1 == self.membership.semantic_root
                && self
                    .membership
                    .active_links
                    .iter()
                    .any(|(owner, _)| *owner == scope.0)
        })
    }

    pub(in crate::mandoc) fn note_unrepresentable_annotation_member(
        &self,
        source: Option<mant_ir::SourceSpan>,
    ) {
        let Some(annotations) = &self.annotations else {
            return;
        };
        for (epoch, target) in &self.membership.active_links {
            if self
                .membership
                .output_owners
                .iter()
                .any(|scope| scope.0 == *epoch && scope.1 == self.membership.semantic_root)
            {
                annotations.note_unrepresentable_member(target, source);
            }
        }
    }

    pub(in crate::mandoc) fn close_annotation_root(&mut self) -> Vec<String> {
        let scopes: Vec<_> = self
            .membership
            .output_owners
            .iter()
            .filter(|scope| {
                scope.1 == self.membership.semantic_root
                    && self
                        .membership
                        .active_links
                        .iter()
                        .any(|(owner, _)| *owner == scope.0)
            })
            .copied()
            .collect();
        // Normal recursive return binds the innermost source first. A real
        // root close must build the same decoration order for still-pending
        // cells, so an outer partial annotation cannot erase the inner label.
        for &(epoch, root) in scopes.iter().rev() {
            if let Some((_, target)) = self
                .membership
                .active_links
                .iter()
                .find(|(owner, _)| *owner == epoch)
            {
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
        self.membership.semantic_root = self
            .membership
            .semantic_root
            .checked_add(1)
            .expect("bounded annotation roots");
        scopes
            .into_iter()
            .map(|scope| markers::end_marker(scope.0))
            .collect()
    }
    /// A semantic wrapper can begin while BACKBEFORE still owns a glyph from
    /// preceding source. Track that glyph through the shared formatter until
    /// it is emitted or overstruck, including across nested wrappers.
    pub(in crate::mandoc) fn begin_output_owner(&mut self) {
        self.membership.output_epoch = self
            .membership
            .output_epoch
            .checked_add(1)
            .expect("bounded annotation entries");
        self.membership
            .output_owners
            .push((self.membership.output_epoch, self.membership.semantic_root));
    }

    pub(in crate::mandoc) fn end_output_owner(&mut self) -> OutputMembership {
        let (epoch, root) = self
            .membership
            .output_owners
            .pop()
            .expect("balanced annotation owner");
        self.membership
            .active_links
            .retain(|(owner, _)| *owner != epoch);
        OutputMembership {
            epoch,
            root,
            enclosing: self.membership.output_owners.last().map(|scope| scope.0),
        }
    }

    /// Positive leading motion is emitted outside all active annotations
    /// which have yet to receive an authored member. Older enclosing labels
    /// may already own the same space as an internal separator.
    pub(in crate::mandoc) fn leading_motion_origin(&self) -> Option<u64> {
        self.membership
            .output_owners
            .iter()
            .find(|(epoch, root)| {
                *epoch > self.membership.authored_member_epoch
                    && *root == self.membership.semantic_root
            })
            .map(|(epoch, _)| epoch - 1)
    }

    pub(in crate::mandoc) fn note_authored_member(&mut self) {
        self.membership.authored_member_epoch = self.membership.output_epoch;
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
