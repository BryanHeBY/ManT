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

    /// Preview this scope's unresolved candidate before native acceptance.
    /// Held blanks belong to that candidate even when a later retreat or
    /// retirement removes them. The final receipt must prove their survival.
    pub(in crate::mandoc) fn deferred_member_projection(
        &self,
        scope: OutputMembership,
    ) -> Vec<mant_ir::Inline> {
        let deferred = self.machine.deferred_projection();
        let first = deferred.partition_point(|glyph| glyph.epoch < scope.epoch);
        let pending = self.machine.pending_projection();
        let split = pending.map_or(deferred.len(), |(position, _)| position.max(first));
        let held_first = self.held.partition_point(|cell| cell.epoch < scope.epoch);
        let mut held = self.held[held_first..]
            .iter()
            .filter(|cell| cell.root == scope.root)
            .peekable();
        #[cfg(test)]
        crate::mandoc::inline::ownership::metrics::note(|counts| {
            counts.previewed +=
                deferred.len() - first + usize::from(pending.is_some()) + self.held.len()
                    - held_first;
        });
        let glyphs = deferred[first..split]
            .iter()
            .enumerate()
            .map(|(index, glyph)| ((first + index, 2), glyph))
            .chain(pending.map(|(position, glyph)| ((position, 0), glyph)))
            .chain(
                deferred[split..]
                    .iter()
                    .enumerate()
                    .map(|(index, glyph)| ((split + index, 2), glyph)),
            )
            .filter(|(_, glyph)| glyph.epoch >= scope.epoch && glyph.root == scope.root);
        let held_glyph = |cell: &super::HeldCell| super::OwnedGlyph {
            value: super::styled_link(cell.character.to_string(), cell.font, cell.link.as_deref()),
            decoration: cell.decoration.clone(),
            owner: cell.owner,
            link_continuation: cell.link_continuation,
            epoch: cell.epoch,
            root: cell.root,
        };
        let mut candidate = Vec::new();
        // take_deferred_with_held() emits a buffered glyph, then held blanks,
        // then recovery spelling at the same recovery offset. Skip all older
        // source prefixes before merging these scope-owned suffixes.
        for (position, glyph) in glyphs {
            while held
                .peek()
                .is_some_and(|cell| (cell.recovery_offset, 1) < position)
            {
                candidate.push(held_glyph(held.next().expect("held candidate")));
            }
            candidate.push(glyph.clone());
        }
        candidate.extend(held.map(held_glyph));
        let mut owned = Vec::new();
        scope.map_owned(self.project_owned(candidate), |children| {
            owned.extend(children);
            Vec::new()
        });
        owned
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

#[cfg(test)]
mod preview_tests {
    use super::super::{Font, HeldCell, Inline, OwnedGlyph};
    use super::*;
    use crate::mandoc::inline::ownership::metrics;

    #[derive(Clone, Copy, Debug)]
    enum Pending {
        Absent,
        Projected,
        Before,
        Middle,
        Foreign,
    }

    fn glyph(epoch: u64, value: &str) -> OwnedGlyph {
        OwnedGlyph {
            value: Inline::Text {
                value: value.into(),
            },
            decoration: None,
            owner: Some(epoch),
            link_continuation: false,
            epoch,
            root: 7,
        }
    }

    fn state(history: usize, pending: Pending) -> ZeroAdvanceState {
        let mut state = ZeroAdvanceState::new();
        // An enclosing receiver is still active after the equation's own
        // receiver ended. Old foreign members retain their birth receipts.
        state.membership.output_owners.push((2, 7));
        state.membership.semantic_root = 7;
        state.membership.output_epoch = 3;
        state.native_word_owner = Some(99);
        if matches!(pending, Pending::Projected) {
            state.machine.arm();
            let _ = state.machine.project_glyph(glyph(1, "projected"));
            let _ = state.machine.commit_pending_projection();
        }
        for _ in 0..history {
            state.machine.defer_projection(glyph(1, "foreign"));
            state.held.push(HeldCell {
                character: ' ',
                font: Font::Regular,
                link: None,
                owner: Some(1),
                recovery_offset: 0,
                epoch: 1,
                root: 7,
                decoration: None,
                link_continuation: false,
            });
        }
        if matches!(pending, Pending::Middle) {
            state.machine.defer_projection(glyph(3, "L"));
        }
        if matches!(
            pending,
            Pending::Before | Pending::Middle | Pending::Foreign
        ) {
            state.machine.arm();
            let _ = state.machine.project_glyph(glyph(
                if matches!(pending, Pending::Foreign) {
                    1
                } else {
                    3
                },
                "P",
            ));
        }
        if !matches!(pending, Pending::Middle) {
            if state.machine.has_pending() {
                let _ = state.machine.project_fallback(glyph(3, "L"));
            } else {
                state.machine.defer_projection(glyph(3, "L"));
            }
        }
        if state.machine.has_pending() {
            let _ = state.machine.project_fallback(glyph(3, "R"));
        } else {
            state.machine.defer_projection(glyph(3, "R"));
        }
        let mut own_blank = state.held.last().cloned().unwrap_or(HeldCell {
            character: ' ',
            font: Font::Regular,
            link: None,
            owner: Some(3),
            recovery_offset: 0,
            epoch: 3,
            root: 7,
            decoration: None,
            link_continuation: false,
        });
        own_blank.epoch = 3;
        own_blank.owner = Some(3);
        state.held.push(own_blank);
        state
    }

    #[test]
    fn scoped_pending_preview_preserves_order_without_copying_foreign_history() {
        // Private candidate-order and work test, independent of new roff
        // behavior. Native acceptance later decides whether held blanks live.
        let scope = OutputMembership {
            epoch: 3,
            root: 7,
            enclosing: Some(2),
        };
        for pending in [
            Pending::Absent,
            Pending::Projected,
            Pending::Before,
            Pending::Middle,
            Pending::Foreign,
        ] {
            for history in [0, 64, 4096] {
                let live = state(history, pending);
                let original = live.clone();
                let mut previous = live.clone();
                let settled = previous.take_deferred_with_held();
                let mut expected = Vec::new();
                scope.map_owned(settled, |nodes| {
                    expected.extend(nodes);
                    Vec::new()
                });
                metrics::take();
                let actual = live.deferred_member_projection(scope);
                let work = metrics::take();
                // Foreign field-word anchors can divide equal plain runs in
                // the full-state control. Compare exact candidate content.
                assert_eq!(
                    mant_ir::inline_plain_text(&actual),
                    mant_ir::inline_plain_text(&expected),
                    "{pending:?}, history {history}"
                );
                assert_eq!(live, original, "preview must preserve the live machine");
                assert_eq!(
                    work.previewed,
                    match pending {
                        Pending::Absent | Pending::Projected => 3,
                        Pending::Before | Pending::Middle | Pending::Foreign => 4,
                    },
                    "{pending:?}, history {history}"
                );
                assert!(!mant_ir::inline_plain_text(&actual).contains("foreign"));
                assert_eq!(mant_ir::inline_plain_text(&actual).matches(' ').count(), 1);
            }
        }
    }
}
