//! Literal translation of the pinned CVS `term.c` line buffer and its
//! `term_fill()` pass simulation.
//!
//! Upstream decides every definition-field row from ONE flat cell buffer at
//! `term_flushln()` time; streaming approximations at word boundaries
//! provably diverge (a `\z` glyph's `TERMP_BACKBEFORE` retreat eats the blank
//! before the NEXT glyph, an empty operand emits a real separator blank,
//! and a rejected pass clears the whole unprinted remainder). This module
//! records those cells in source order and answers the pass questions the
//! renderer asked, with the same record/break/overflow arithmetic.
//!
/// Reference: term.c v1.295 — `bufferc()` 857-870, `term_flushln()` pass
/// loop 123-231 with the nbr==0 break 143-146, blank consumption 205-207,
/// BRIND continuation 229-230, and reset 233-237; `term_fill()` 263-367
/// (`breakline`/`graph` locals, blank record 296-300, break 294, graph
/// overflow early return 350-351, tail acceptance 362-366); `term_field()`
/// 374-444; `encode1()` BACKBEFORE retreat 901-908 (blank: `col--`,
/// otherwise buffer `'\b'`); `term_word()` separator blanks 573-576.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FieldCell {
    /// One printable graph with its terminal width.
    Graph { text: char, width: usize },
    /// An ordinary breakable blank (`bufferc(' ')`, term.c:576 and 574-576
    /// for empty operands).
    BreakableBlank,
    /// A non-breaking blank (`ASCII_NBRSP`: `\~`, `\0`, KEPT separators).
    /// `term_fill` counts its width (342-347) and never breaks on it.
    NonBreakingBlank,
    /// A `\p` break marker (`bufferc('\n')`, term.c:657-658). A pass only
    /// arms its LOCAL `breakline` from it (304-306); `term_field` skips it.
    BreakMarker,
    /// A zero-width breakpoint `\:` (`ASCII_BREAK`, term.c:287-300).
    /// Shares the breakable-blank arm with no width of its own: a pass may
    /// break at it, records it as the resume candidate after a graph, and
    /// never prints it (term.c:396-398).
    Breakpoint,
    /// The `'\b'` `encode1()` buffers when a BACKBEFORE retreat meets a
    /// non-blank predecessor (term.c:906): fill subtracts the width of the
    /// cell before it (283-286), then the following graph adds its own.
    Backline,
}

/// One `term_fill()` result: the slice accepted for the current output
/// line (`nbr` bytes, `vbr` visual width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FillPass {
    /// term.c `nbr`: buffer index ENDING the accepted slice (exclusive).
    pub(super) accepted_end: usize,
    /// term.c `vbr`: visual width of the accepted slice.
    pub(super) accepted_width: usize,
}

/// The unflushed input field of `term.c::term_flushln()`, kept across the
/// words of one native field.
#[derive(Clone, Debug, Default)]
pub(super) struct FieldBuffer {
    cells: Vec<FieldCell>,
    /// `p->tcol->col`: the next pass resumes here (`term_field` advances it
    /// by `nbr`; 205-207 then consumes break blanks; 235 resets it).
    resume: usize,
    /// A completed `\z` glyph left `TERMP_BACKBEFORE` armed: the next graph
    /// retreats over the cell before it (term.c:901-908).
    backbefore_armed: bool,
}

impl FieldBuffer {
    pub(super) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// term.c:233-237: the row ends and the buffer restarts empty (used
    /// both by the accepted-exit and by the nbr==0 wipe).
    pub(super) fn clear(&mut self) {
        self.cells.clear();
        self.resume = 0;
        self.backbefore_armed = false;
    }

    /// `term_word()`'s inter-word blank (term.c:574-576), and the real
    /// blank an empty operand still emits: ordinary breakable cells.
    pub(super) fn push_separator_blank(&mut self) {
        self.cells.push(FieldCell::BreakableBlank);
    }

    /// `bufferc('\n')` for `\p` (term.c:657-658).
    pub(super) fn push_break_marker(&mut self) {
        self.cells.push(FieldCell::BreakMarker);
    }

    /// `bufferc(ASCII_BREAK)` for `\:` (term.c:287-300).
    pub(super) fn push_breakpoint(&mut self) {
        self.cells.push(FieldCell::Breakpoint);
    }

    /// A fixed-width non-breaking cell: generated `\ ` run-in gaps and
    /// `\~`/`\0` blanks (mdoc_term.c:760-767; printed as spaces at
    /// term.c:342-344).
    pub(super) fn push_non_breaking_blank(&mut self) {
        self.cells.push(FieldCell::NonBreakingBlank);
    }

    /// `encode1()` writes a graph (term.c:345-349 tail with 901-908): a
    /// pending `TERMP_BACKBEFORE` retreat first consumes the blank directly
    /// before it (`col--`), or buffers `'\b'` over a non-blank cell.
    pub(super) fn push_graph(&mut self, text: char, width: usize) {
        if self.backbefore_armed {
            self.backbefore_armed = false;
            match self.cells.last() {
                Some(FieldCell::BreakableBlank) => {
                    self.cells.pop();
                }
                Some(FieldCell::Graph { .. } | FieldCell::NonBreakingBlank) => {
                    self.cells.push(FieldCell::Backline);
                }
                _ => {}
            }
        }
        self.cells.push(FieldCell::Graph { text, width });
    }

    /// Arm the retreat for the NEXT glyph after a completed `\z` glyph
    /// (encode1 tail, term.c:924-927: BACKAFTER converts to BACKBEFORE).
    pub(super) fn arm_backbefore(&mut self) {
        self.backbefore_armed = true;
    }

    /// `term_flushln()` clears both backtracking flags with the row
    /// (term.c:235-237).
    pub(super) fn clear_backtracking(&mut self) {
        self.backbefore_armed = false;
    }

    /// One `term_fill()` pass from `resume` (term.c:263-367).
    ///
    /// Returns `None` for the nbr=0 rejection (143-146): nothing in this
    /// slice may print and the caller wipes the remainder. Otherwise the
    /// accepted slice is `cells[..accepted_end]`; the breaking blank (if
    /// any) sits at `accepted_end` and is consumed separately by
    /// [`Self::consume_break_blanks`] (term.c:205-207).
    pub(super) fn fill_pass(&self, vtarget: usize) -> Option<FillPass> {
        let enw = 1usize;
        // term.c:280: half an EN of grace on the target.
        let vtarget = vtarget + enw / 2;
        let mut nbr = 0usize;
        let mut vbr = 0usize;
        let mut vis = 0usize;
        let mut breakline = false;
        let mut graph = false;
        let mut ic = self.resume;
        while ic < self.cells.len() {
            match self.cells[ic] {
                FieldCell::Backline => {
                    // term.c:283-286: overstrike subtracts the previous
                    // cell's width.
                    let previous = ic
                        .checked_sub(1)
                        .and_then(|index| match self.cells[index] {
                            FieldCell::Graph { width, .. } => Some(width),
                            FieldCell::NonBreakingBlank => Some(1),
                            _ => None,
                        })
                        .unwrap_or(0);
                    vis = vis.saturating_sub(previous);
                    ic += 1;
                }
                FieldCell::Breakpoint => {
                    // term.c:287-300: ASCII_BREAK keeps `vn = vis` (only a
                    // real ' ' gains enw) but otherwise shares the blank
                    // arm: break under an armed marker or past the target,
                    // else record the candidate after a graph.
                    let vn = vis;
                    if breakline || vn > vtarget {
                        break;
                    }
                    if graph {
                        nbr = ic;
                        vbr = vis;
                        graph = false;
                    }
                    ic += 1;
                }
                FieldCell::BreakableBlank => {
                    let vn = vis + enw;
                    // term.c:294: break at the word end under an armed
                    // marker, or past the target.
                    if breakline || vn > vtarget {
                        break;
                    }
                    if graph {
                        // term.c:296-300: record this blank as the next
                        // resume candidate.
                        nbr = ic;
                        vbr = vis;
                        graph = false;
                    }
                    vis = vn;
                    ic += 1;
                }
                FieldCell::BreakMarker => {
                    // term.c:304-306: local breakline only.
                    breakline = true;
                    ic += 1;
                }
                FieldCell::NonBreakingBlank => {
                    vis += 1;
                    graph = true;
                    if vis > vtarget && nbr > 0 {
                        // term.c:350-351.
                        return Some(FillPass {
                            accepted_end: nbr,
                            accepted_width: vbr,
                        });
                    }
                    ic += 1;
                }
                FieldCell::Graph { width, .. } => {
                    vis += width;
                    graph = true;
                    if vis > vtarget && nbr > 0 {
                        // term.c:350-351: an overrun graph ends the pass at
                        // the last recorded blank.
                        return Some(FillPass {
                            accepted_end: nbr,
                            accepted_width: vbr,
                        });
                    }
                    ic += 1;
                }
            }
        }
        // term.c:362-366: the final word runs to the buffer end, or the
        // slice never found a stop.
        if graph && (vis <= vtarget || nbr == 0) {
            nbr = ic;
            vbr = vis;
        }
        if nbr == 0 {
            return None;
        }
        Some(FillPass {
            accepted_end: nbr,
            accepted_width: vbr,
        })
    }

    /// term.c:205-207: input blanks at the resume point are consumed by
    /// the automatic line break itself.
    pub(super) fn consume_break_blanks(&mut self) {
        while self.resume < self.cells.len()
            && matches!(self.cells[self.resume], FieldCell::BreakableBlank)
        {
            self.resume += 1;
        }
    }

    /// The nbr=0 wipe (term.c:233-237 reached through 145-146): the whole
    /// unprinted remainder dies with the row.
    pub(super) fn wipe_remainder(&mut self) {
        self.clear();
    }

    /// `term_field()` committed `nbr` cells and advanced `col`
    /// (term.c:443); drop them so later passes index the remainder.
    pub(super) fn advance_past(&mut self, accepted_end: usize) {
        self.resume = self.resume.max(accepted_end.min(self.cells.len()));
    }

    /// term.c:177-198: whether anything but ignorable cells (blanks,
    /// markers; trailing blanks under BRTRSP count) remains unprinted.
    pub(super) fn only_ignorable_remainder(&self, brtrsp: bool) -> bool {
        self.cells[self.resume.min(self.cells.len())..]
            .iter()
            .all(|cell| match cell {
                FieldCell::BreakableBlank => !brtrsp,
                FieldCell::BreakMarker | FieldCell::Backline => true,
                _ => false,
            })
    }

    /// Whether a completed `\z` glyph's retreat is still armed for the
    /// next graph (term.c:901-908 decides at the graph, not the blank).
    pub(super) fn backbefore_armed(&self) -> bool {
        self.backbefore_armed
    }

    pub(super) fn cells(&self) -> &[FieldCell] {
        &self.cells
    }

    pub(super) fn resume_offset(&self) -> usize {
        self.resume
    }
}

#[cfg(test)]
mod term_fill_contract_tests {
    use super::FieldBuffer;

    /// (a) of the fixed-CVS oracle pair: `\zX\p` + `\p Y` keeps Y. The
    /// retreat eats the blank before Y, so pass two resumes at the second
    /// marker and accepts Y itself (term.c:901-908 with 263-367; pass one
    /// accepts `X` + marker, nbr = the breaking blank's index).
    #[test]
    fn retreat_blank_keeps_the_following_graph() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('X', 1);
        buffer.arm_backbefore();
        buffer.push_break_marker(); // \p of word one
        buffer.push_separator_blank(); // word two's separator
        buffer.push_break_marker(); // \p of word two
        buffer.push_separator_blank(); // " Y" operand blank
        buffer.push_graph('Y', 1); // retreat eats the operand blank
        let first = buffer
            .fill_pass(usize::MAX / 2)
            .expect("pass one accepts X");
        assert_eq!(first.accepted_end, 2, "X + marker; blank breaks");
        buffer.advance_past(first.accepted_end);
        buffer.consume_break_blanks();
        let second = buffer
            .fill_pass(usize::MAX / 2)
            .expect("pass two accepts Y");
        assert!(second.accepted_end >= 4);
        assert_eq!(second.accepted_width, 1);
    }

    /// (b) of the pair: the empty operand's real blank survives after the
    /// marker, so the next pass stops graphless at it (nbr stays 0 through
    /// term.c:362-366) and the remainder is wiped (143-146 with 233-237).
    #[test]
    fn blank_after_marker_rejects_and_wipes() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('X', 1);
        buffer.arm_backbefore();
        buffer.push_break_marker(); // word one's \p
        buffer.push_separator_blank(); // word two's separator
        buffer.push_break_marker(); // word two's \p
        buffer.push_separator_blank(); // `.No ""` term_word("") blank
        buffer.push_separator_blank(); // Y's own word separator
        buffer.push_graph('Y', 1); // retreat eats only Y's own blank
        let first = buffer
            .fill_pass(usize::MAX / 2)
            .expect("pass one accepts X");
        buffer.advance_past(first.accepted_end);
        buffer.consume_break_blanks();
        assert!(
            buffer.fill_pass(usize::MAX / 2).is_none(),
            "the surviving blank under breakline rejects the pass"
        );
        buffer.wipe_remainder();
        assert!(buffer.is_empty());
    }

    /// An armed marker with no graph before the stop also rejects: `\p`
    /// alone before a separator blank never prints (term.c:294 with
    /// 362-366 leaving nbr at 0).
    #[test]
    fn armed_marker_without_graph_rejects() {
        let mut buffer = FieldBuffer::default();
        buffer.push_break_marker();
        buffer.push_separator_blank();
        buffer.push_graph('X', 1);
        assert!(buffer.fill_pass(usize::MAX / 2).is_none());
    }

    /// term.c:350-351: a graph that overruns the target returns at the
    /// last recorded blank; the blank itself is consumed with the break
    /// (205-207), so the next pass starts at the overflowing word.
    #[test]
    fn vtarget_overrun_breaks_at_the_last_recorded_blank() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('a', 1);
        buffer.push_separator_blank();
        buffer.push_graph('b', 1);
        buffer.push_separator_blank();
        buffer.push_graph('c', 1);
        let pass = buffer.fill_pass(2).expect("breaks inside the slice");
        assert_eq!(pass.accepted_end, 1, "accepts `a`; blank consumed");
        buffer.advance_past(pass.accepted_end);
        buffer.consume_break_blanks();
        assert_eq!(buffer.resume_offset(), 2, "next pass starts at `b`");
    }

    /// term.c:362-366: a word running to the buffer end is accepted whole,
    /// even past the target (nbr==0 has no recorded candidate to return
    /// to; the 350-351 guard only fires when one exists).
    #[test]
    fn trailing_word_is_accepted_whole() {
        let mut buffer = FieldBuffer::default();
        buffer.push_separator_blank();
        buffer.push_graph('a', 1);
        buffer.push_graph('b', 1);
        let pass = buffer.fill_pass(1).expect("whole word accepted");
        assert_eq!(pass.accepted_end, 3);
        assert_eq!(pass.accepted_width, 3, "the leading blank counts width");
    }

    /// A non-breaking blank counts width and never breaks
    /// (`ASCII_NBRSP` through the default branch, term.c:342-347).
    #[test]
    fn non_breaking_blank_counts_width_without_breaking() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('a', 1);
        buffer.push_non_breaking_blank();
        buffer.push_graph('b', 1);
        let pass = buffer.fill_pass(1).expect("nbr does not stop the pass");
        assert_eq!(pass.accepted_end, 3);
        assert_eq!(pass.accepted_width, 3);
    }

    /// term.c:283-286 with 906: a BACKBEFORE retreat over a graph buffers
    /// `'\b'`; fill subtracts the predecessor's width before the new graph
    /// adds its own, so a `\z` overstrike keeps one column.
    #[test]
    fn backline_retreat_subtracts_the_previous_width() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('X', 1);
        buffer.arm_backbefore();
        buffer.push_graph('Y', 1); // buffers '\b' over X
        assert_eq!(
            buffer.cells(),
            &[
                super::FieldCell::Graph {
                    text: 'X',
                    width: 1
                },
                super::FieldCell::Backline,
                super::FieldCell::Graph {
                    text: 'Y',
                    width: 1
                },
            ][..]
        );
        let pass = buffer.fill_pass(usize::MAX / 2).expect("accepted");
        assert_eq!(pass.accepted_width, 1, "overstrike keeps one column");
    }
}
