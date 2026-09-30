/// Row geometry a control request left behind in a definition head.
///
/// Upstream, the request's `roff_term_pre_br()` moves the device row
/// origin (`offset <- rmargin`, roff_term.c:73-75) and the roff node
/// escapes the document save/restore (mdoc_term.c:393-397), so the
/// geometry lives until the next document node boundary restores the
/// authored values (mdoc_term.c:329-330, 437-439). A HANG row stays open
/// through the request (`roff_term.c:76`), so the next word prints
/// through `vbl = offset - viscol` (term.c:113-114) - a horizontal jump -
/// and upstream defers that word's flush until either a later input-row
/// event prints it (the jump stands) or the head close does, after the
/// restore (the jump collapses). This machine models that lifetime
/// instead of scanning output nodes after the fact.
#[derive(Clone, Debug, Default)]
pub(in crate::mandoc::inline::flow) struct HeadRowState {
    /// Indent (columns relative to the field origin) the row a break
    /// starts carries - the tag path's row origin after the request.
    pub(in crate::mandoc::inline::flow) indent_columns: u16,
    /// This output owner has a following-row hint awaiting its first flush.
    /// This is an IR target, not native row occupancy or field acceptance.
    origin_pending: bool,
    /// Row offset (`rmargin` relative to the field origin) a request
    /// armed for the open HANG row: the fill at print time is
    /// `offset - viscol` (term.c:113-114), computed when the carrying
    /// word emits, because viscol keeps advancing until then.
    armed_offset_columns: u16,
    /// An emitted jump awaiting its word's print timing.
    pending: Option<PendingRowJump>,
}

#[derive(Clone, Copy, Debug)]
struct PendingRowJump {
    /// Node index of the emitted fill text.
    node: usize,
    /// The word carrying the fill finished appending.
    word_closed: bool,
}

impl HeadRowState {
    pub(in crate::mandoc::inline::flow) fn note_row_origin(&mut self) {
        self.origin_pending = true;
    }

    pub(in crate::mandoc::inline::flow) const fn has_pending_origin(&self) -> bool {
        self.origin_pending
    }

    pub(in crate::mandoc::inline::flow) fn retire_row_origin(&mut self) {
        self.origin_pending = false;
    }

    /// Arm the request-moved row offset for the next word on the open
    /// row.
    pub(in crate::mandoc::inline::flow) fn arm_jump(&mut self, offset_columns: u16) {
        self.armed_offset_columns = offset_columns;
    }

    /// Emit the armed jump as `offset - viscol` fill at `node`, carried
    /// by the word currently appending.
    pub(in crate::mandoc::inline::flow) fn emit_armed(&mut self, node: usize, viscol: u16) -> u16 {
        let columns = self.armed_offset_columns.saturating_sub(viscol);
        self.armed_offset_columns = 0;
        if columns > 0 {
            self.pending = Some(PendingRowJump {
                node,
                word_closed: false,
            });
        }
        columns
    }

    /// The word carrying an emitted jump finished appending.
    pub(in crate::mandoc::inline::flow) fn close_word(&mut self) {
        if let Some(pending) = &mut self.pending {
            pending.word_closed = true;
        }
    }

    /// A source-row flush prints the carrying word before the enclosing
    /// node restores its offset. Commit the emitted jump at that execution
    /// boundary; another word arriving alone does not commit it.
    pub(in crate::mandoc::inline::flow) fn commit_on_source_flush(&mut self) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.word_closed)
        {
            self.pending = None;
        }
    }

    /// The head closed with a jump still uncommitted: upstream prints the
    /// buffered word only after the restore zeroed the offset, so the
    /// jump collapses. Returns the node whose fill retracts.
    pub(in crate::mandoc::inline::flow) fn retract_on_head_close(&mut self) -> Option<usize> {
        self.pending.take().map(|pending| pending.node)
    }
}
#[cfg(test)]
mod head_row_state_tests {
    use super::HeadRowState;

    // Every case below pins a print-deferral fact verified against the
    // fixed CVS reference by the engine roff_lowering contracts
    // (definition_fields.rs: hang fill boundary fit/overrun, the bare-zero
    // handoff, and the wrapping probes); these unit tests keep the state
    // machine's own lifecycle observable.

    #[test]
    fn a_later_word_commits_the_jump() {
        // LONGTEXT .mc .nf A An -split An Bob: 'A' emits the fill, 'Bob's
        // row event prints it before the restore — the fill stands.
        let mut row = HeadRowState::default();
        row.arm_jump(14);
        assert_eq!(row.emit_armed(0, 8), 6);
        row.close_word();
        row.commit_on_source_flush();
        assert_eq!(row.retract_on_head_close(), None);
    }

    #[test]
    fn the_item_post_retracts_an_uncommitted_jump() {
        // LONGTEXT .mc .No \z .nf An -split An Bob: 'Bob' is the only word
        // after the boundary; the item post prints it past the element
        // restore (mdoc_term.c:437-439) and the fill collapses.
        let mut row = HeadRowState::default();
        row.arm_jump(14);
        assert_eq!(row.emit_armed(0, 8), 6);
        row.close_word();
        assert_eq!(row.retract_on_head_close(), Some(0));
    }

    #[test]
    fn a_width_exact_head_emits_no_fill() {
        // 6n == len(LONGTEXT): offset == viscol at print time, vbl = 0.
        let mut row = HeadRowState::default();
        row.arm_jump(8);
        assert_eq!(row.emit_armed(0, 8), 0);
        assert_eq!(row.retract_on_head_close(), None);
    }

    #[test]
    fn an_undelivered_arm_does_not_retract_text() {
        // Nothing emitted: no node index exists to clear.
        let mut row = HeadRowState::default();
        row.arm_jump(6);
        assert_eq!(row.emit_armed(0, 8), 0);
        row.close_word();
        assert_eq!(row.retract_on_head_close(), None);
    }
}
