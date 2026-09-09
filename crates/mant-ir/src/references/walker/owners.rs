//! Paired physical/semantic ancestry; transparent items change only the former.
use super::OwnerFrame;

#[derive(Clone, Copy, Default)]
pub(super) struct OwnerFrames<'ir> {
    content: Option<OwnerFrame<'ir>>,
    semantic: Option<OwnerFrame<'ir>>,
}

impl<'ir> OwnerFrames<'ir> {
    /// Save the prior pair for a child visit, or discard it when reconstructing
    /// the ancestors of one explicitly selected root. Paths remain in Scan.
    pub(super) fn enter(&mut self, frame: OwnerFrame<'ir>) -> Self {
        let previous = *self;
        self.content = Some(frame);
        if frame.owner.facts().is_some() {
            self.semantic = Some(frame);
        }
        previous
    }

    pub(super) fn restore(&mut self, previous: Self) {
        *self = previous;
    }

    pub(super) const fn content(self) -> Option<OwnerFrame<'ir>> {
        self.content
    }

    pub(super) const fn semantic(self) -> Option<OwnerFrame<'ir>> {
        self.semantic
    }
}
