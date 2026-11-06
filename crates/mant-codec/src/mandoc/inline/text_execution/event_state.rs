use super::{Inline, TextEventState, WordOutput, is_formatter_word_blank};

impl TextEventState {
    pub(super) fn note_text_character(&mut self, character: char) {
        let graph = !is_formatter_word_blank(character) && character != '\n';
        self.graph_count += usize::from(graph);
        self.graph_seen |= graph;
        self.graph_since_break |= self.pending_word_end_break && graph;
        self.graph_since_blank = graph;
        self.saw_source_cell = true;
        self.last_breakable_blank = is_formatter_word_blank(character);
        self.trailing_breakable_blanks = if self.last_breakable_blank {
            self.trailing_breakable_blanks.saturating_add(1)
        } else {
            0
        };
    }

    pub(super) fn note_graph(&mut self) {
        self.graph_seen = true;
        self.graph_since_blank = true;
        self.graph_since_break |= self.pending_word_end_break;
        self.last_breakable_blank = false;
        self.trailing_breakable_blanks = 0;
    }

    pub(super) const fn new(pending_word_end_break: bool) -> Self {
        Self {
            pending_word_end_break,
            suppress_break_whitespace: false,
            graph_seen: false,
            last_breakable_blank: false,
            saw_source_cell: false,
            trailing_breakable_blanks: 0,
            graph_since_blank: false,
            break_started_after_blank: false,
            break_trailing_blanks: 0,
            graph_since_break: false,
            graph_count: 0,
            wiped: false,
            zero_graph_seen: false,
            projected_word_separator: 0,
        }
    }
}

pub(super) fn append_text_event(
    value: &str,
    destination: WordOutput<'_>,
    state: &mut TextEventState,
    field_authoritative: bool,
) {
    let WordOutput {
        output,
        buffer,
        font,
        link,
        zero_advance,
    } = destination;
    if !state.pending_word_end_break && !state.suppress_break_whitespace && !state.wiped {
        for character in value.chars() {
            state.note_text_character(character);
        }
        // With no break-driven filtering, the chunk is byte-identical to
        // the decoded event. Zero-advance still consumes the borrowed run.
        zero_advance.append_text(value, output, buffer, font, link);
        return;
    }
    let mut chunk = String::new();
    for character in value.chars() {
        if state.wiped {
            // The flush unit's remainder is unprinted input (term.c:233-237
            // reached through 143-146); graphs and blanks alike die with it.
            continue;
        }
        if state.pending_word_end_break && is_formatter_word_blank(character) {
            zero_advance.append_text(&chunk, output, buffer, font, link);
            chunk.clear();
            // This is the marker's first following blank in either
            // projection arm. BACKBEFORE can consume that blank, but it
            // cannot make a later blank in the same word another marker
            // event (term.c:294-305,901-908). Native passes retain breakline
            // independently until they accept their ordered cell interval.
            state.pending_word_end_break = false;
            if zero_advance.has_buffered_glyph() {
                // CVS stores `\\p` in the same terminal buffer as a
                // completed `\\z` glyph.  The intervening word blank settles
                // that glyph first; the next graph's retreat consumes the
                // marker's own blank (term.c:901-908), so the blank held
                // before the marker — like the word separator of the `ph`
                // shape — survives and prints after the settled glyph
                // (term.c:573-576).  `breakline` stays armed: term_fill()
                // cannot stop at the eaten blank and defers the break to
                // the next surviving blank (term.c:294-295).
                zero_advance.flush_with_held(output, buffer, font, link);
                let cells = zero_advance.take_marker_blank_separator();
                buffer.extend(std::iter::repeat_n(' ', cells));
                state.projected_word_separator =
                    state.projected_word_separator.saturating_add(cells);
                state.suppress_break_whitespace = true;
                continue;
            }
            zero_advance.flush(output, buffer, font, link);
            let rejected_after_accepted_blank =
                state.break_started_after_blank && !state.graph_since_break;
            if rejected_after_accepted_blank {
                // term_fill() records nbr before the breakable blank. That
                // blank is outside the accepted field as well as its suffix.
                crate::mandoc::inline::flow::trim_trailing_breakable_spaces(
                    output,
                    state.break_trailing_blanks,
                );
            }
            // The marker arms `breakline` and this blank stops the pass
            // (term.c:294-295). A definition-field session computes its own
            // pass arithmetic (`FieldBuffer::flush_receipt`); there the
            // committed row keeps its break exactly as before. In ordinary
            // flow, with the `graph` flag still armed — a graph occurred
            // since the last breakable blank — the stopped pass
            // tail-accepts through the marker (term.c:362-366): the row
            // commits, the blank is consumed with the break (205-207), and
            // the NEXT pass continues normally after it. Otherwise the next
            // pass resumes at the marker itself, immediately rejects with
            // `nbr == 0`, and term_flushln() wipes the whole unprinted
            // remainder of the flush unit (term.c:143-146 with 233-237); an
            // earlier graph keeps its committed row (term.c:220).
            if !field_authoritative {
                let tail_accepted = state.graph_since_blank;
                if tail_accepted || state.graph_seen {
                    output.push(Inline::line_break());
                }
                if !tail_accepted {
                    state.wiped = true;
                }
            }
            state.suppress_break_whitespace = true;
            state.graph_seen = false;
            state.graph_since_blank = false;
            state.break_started_after_blank = false;
            state.graph_since_break = false;
            state.last_breakable_blank = false;
            state.trailing_breakable_blanks = 0;
            state.break_trailing_blanks = 0;
            continue;
        }
        if state.suppress_break_whitespace && is_formatter_word_blank(character) {
            continue;
        }
        state.suppress_break_whitespace = false;
        state.note_text_character(character);
        chunk.push(character);
    }
    // The decoder has already classified controls. A backslash produced by
    // \e or \[rs] is literal author content.
    zero_advance.append_text(&chunk, output, buffer, font, link);
}
