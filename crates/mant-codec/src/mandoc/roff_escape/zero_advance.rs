/// Formatter-neutral core of CVS `TERMP_BACKAFTER`/`TERMP_BACKBEFORE`.
///
/// Styled terminal output and plain semantic text both use this state so a
/// target/name projection cannot disagree with the visible document about
/// which complete glyph survives `\z`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct ZeroAdvanceMachine<T> {
    armed: bool,
    pending: Option<PendingGlyph<T>>,
    recoveries: Vec<T>,
    recoveries_before_pending: usize,
}

/// BACKBEFORE can survive a pass that already owns the preceding glyph.
/// Its next retreat still consumes a buffered blank, but cannot erase that
/// immutable projection (`term_fill()` with `encode1()`).
#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingGlyph<T> {
    Buffered(T),
    Projected,
}

impl<T> ZeroAdvanceMachine<T> {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            armed: false,
            pending: None,
            recoveries: Vec::new(),
            recoveries_before_pending: 0,
        }
    }

    pub(in crate::mandoc) fn arm(&mut self) {
        self.armed = true;
    }

    pub(in crate::mandoc) const fn is_armed(&self) -> bool {
        self.armed
    }

    pub(in crate::mandoc) const fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(in crate::mandoc) const fn has_projected_pending(&self) -> bool {
        matches!(self.pending, Some(PendingGlyph::Projected))
    }

    /// A real cell already assigned to an earlier word can await the next
    /// word's output assembly without becoming another backtracking glyph.
    pub(in crate::mandoc) fn defer_projection(&mut self, glyph: T) {
        self.recoveries.push(glyph);
        self.recoveries_before_pending = self.recoveries.len();
    }

    pub(in crate::mandoc) fn deferred_projection_mut(&mut self) -> &mut [T] {
        &mut self.recoveries
    }

    pub(in crate::mandoc) fn deferred_projection(&self) -> &[T] {
        &self.recoveries
    }

    pub(in crate::mandoc) const fn pending_ref(&self) -> Option<&T> {
        match &self.pending {
            Some(PendingGlyph::Buffered(glyph)) => Some(glyph),
            Some(PendingGlyph::Projected) | None => None,
        }
    }

    /// Borrow the buffered glyph and its source-order slot among recoveries.
    pub(in crate::mandoc) fn pending_projection(&self) -> Option<(usize, &T)> {
        self.pending_ref()
            .map(|glyph| (self.recoveries_before_pending, glyph))
    }

    /// Change projection metadata without consuming either backtracking flag.
    pub(in crate::mandoc) fn pending_mut(&mut self) -> Option<&mut T> {
        match &mut self.pending {
            Some(PendingGlyph::Buffered(glyph)) => Some(glyph),
            Some(PendingGlyph::Projected) | None => None,
        }
    }

    pub(in crate::mandoc) fn cancel_armed(&mut self) -> bool {
        std::mem::take(&mut self.armed)
    }

    pub(in crate::mandoc) fn project_glyph(&mut self, glyph: T) -> Option<(T, bool)> {
        if self.pending.is_some() {
            self.recoveries_before_pending = self.recoveries.len();
        }
        if self.armed {
            self.armed = false;
            self.pending = Some(PendingGlyph::Buffered(glyph));
            return None;
        }
        let replaced_pending = matches!(self.pending.take(), Some(PendingGlyph::Buffered(_)));
        Some((glyph, replaced_pending))
    }

    /// Unknown SPECIAL/invalid NUMBERED uses `bufferc(ASCII_NBRZW)`, not
    /// `encode1()`: its recovery spelling cannot consume either backtracking
    /// flag (term.c:610-638). Defer only spelling following a pending glyph,
    /// so source order remains stable when that native glyph later survives
    /// or is overstruck. The queue contains this unresolved suffix only.
    pub(in crate::mandoc) fn project_fallback(&mut self, glyph: T) -> Option<(T, bool)> {
        if self.armed {
            return None;
        }
        if self.pending.is_some() {
            self.recoveries.push(glyph);
            return None;
        }
        Some((glyph, false))
    }

    pub(in crate::mandoc) fn take_recoveries(&mut self) -> Vec<T> {
        self.recoveries_before_pending = 0;
        std::mem::take(&mut self.recoveries)
    }

    pub(in crate::mandoc) fn resolve_word_boundary(&mut self) -> Option<Vec<T>> {
        (!self.armed).then(|| self.take_pending()).flatten()
    }

    /// Move the local pending projection without losing its source-order slot.
    pub(in crate::mandoc) fn take_projection_parts(&mut self) -> (Option<(usize, T)>, Vec<T>) {
        let position = self.recoveries_before_pending;
        let pending = match self.pending.take() {
            Some(PendingGlyph::Buffered(glyph)) => Some((position, glyph)),
            _ => None,
        };
        (pending, self.take_recoveries())
    }

    pub(in crate::mandoc) fn take_pending(&mut self) -> Option<Vec<T>> {
        let glyph = self.pending.take()?;
        let position = self.recoveries_before_pending;
        let mut output = self.take_recoveries();
        if let PendingGlyph::Buffered(glyph) = glyph {
            output.insert(position, glyph);
        }
        (!output.is_empty()).then_some(output)
    }

    /// Assign the glyph to an accepted pass while retaining BACKBEFORE for
    /// the next `encode1()` write. Projection ownership and retreat state are
    /// independent even though they normally advance together.
    pub(in crate::mandoc) fn commit_pending_projection(&mut self) -> Option<Vec<T>> {
        if !matches!(self.pending, Some(PendingGlyph::Buffered(_))) {
            return None;
        }
        let output = self.take_pending();
        self.pending = Some(PendingGlyph::Projected);
        output
    }

    pub(in crate::mandoc) fn discard_pending(&mut self) {
        self.pending = None;
        self.recoveries.clear();
        self.recoveries_before_pending = 0;
    }

    pub(in crate::mandoc) fn clear(&mut self) {
        self.armed = false;
        self.discard_pending();
    }
}
