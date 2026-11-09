use super::{Inline, TextEventState, WordOutput, is_formatter_word_blank};
use crate::mandoc::inline::flow::field_buffer::{DeferredSourceBlank, FieldWrite};
use crate::mandoc::inline::font::style::{flush_segment, styled_link};

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
        }
    }
}

pub(super) fn append_text_event(
    value: &str,
    destination: WordOutput<'_>,
    state: &mut TextEventState,
    field_authoritative: bool,
    mut native_writes: Option<&mut Vec<FieldWrite>>,
) {
    let WordOutput {
        output,
        buffer,
        font,
        link,
        zero_advance,
    } = destination;
    if !state.pending_word_end_break && !state.suppress_break_whitespace && !state.wiped {
        if let Some(writes) = native_writes {
            FieldWrite::append_literal(writes, value);
        }
        for character in value.chars() {
            state.note_text_character(character);
        }
        // With no break-driven filtering, the chunk is byte-identical to
        // the decoded event. Zero-advance still consumes the borrowed run.
        zero_advance.append_text(value, output, buffer, font, link);
        return;
    }
    let mut chunk = String::new();
    let mut destination = WordOutput {
        output,
        buffer,
        font,
        link,
        zero_advance,
    };
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if state.wiped {
            // The flush unit's remainder is unprinted input (term.c:233-237
            // reached through 143-146); graphs and blanks alike die with it.
            continue;
        }
        if state.pending_word_end_break && is_formatter_word_blank(character) {
            destination.append_chunk(&chunk);
            chunk.clear();
            // This is the marker's first following blank in either
            // projection arm. BACKBEFORE can consume that blank, but it
            // cannot make a later blank in the same word another marker
            // event (term.c:294-305,901-908). Native passes retain breakline
            // independently until they accept their ordered cell interval.
            state.pending_word_end_break = false;
            if destination.zero_advance.has_buffered_glyph() {
                // Release the cached glyph and held author spaces in
                // their source order. The next encode1() can consume
                // the marker's raw blank, leaving the break pending
                // (term.c:294-295,901-908).
                destination.zero_advance.flush_with_held(
                    destination.output,
                    destination.buffer,
                    font,
                    link,
                );
                // The authored blank is not the automatic word separator.
                // Only the field receipt can decide whether that separator
                // survived the retreat and actually printed in its pass.
                state.suppress_break_whitespace = true;
                if let Some(writes) = native_writes.as_deref_mut() {
                    destination.defer_source_blanks(writes, &mut characters);
                }
                continue;
            }
            destination
                .zero_advance
                .flush(destination.output, destination.buffer, font, link);
            let rejected_after_accepted_blank =
                state.break_started_after_blank && !state.graph_since_break;
            if rejected_after_accepted_blank {
                // term_fill() records nbr before the breakable blank. That
                // blank is outside the accepted field as well as its suffix.
                crate::mandoc::inline::flow::trim_trailing_breakable_spaces(
                    destination.output,
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
                    destination.output.push(Inline::line_break());
                }
                if !tail_accepted {
                    state.wiped = true;
                }
            }
            if let Some(writes) = native_writes.as_deref_mut() {
                destination.defer_source_blanks(writes, &mut characters);
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
            if let Some(writes) = native_writes.as_deref_mut() {
                destination.defer_source_blanks(writes, &mut characters);
            }
            continue;
        }
        state.suppress_break_whitespace = false;
        state.note_text_character(character);
        if let Some(writes) = native_writes.as_deref_mut() {
            FieldWrite::append_character(writes, character);
        }
        chunk.push(character);
    }
    // The decoder has already classified controls. A backslash produced by
    // \e or \[rs] is literal author content.
    destination.append_chunk(&chunk);
}

impl WordOutput<'_> {
    fn append_chunk(&mut self, chunk: &str) {
        self.zero_advance
            .append_text(chunk, self.output, self.buffer, self.font, self.link);
    }

    fn defer_source_blanks(
        &mut self,
        writes: &mut Vec<FieldWrite>,
        characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ) {
        let mut count = 1;
        while characters.next_if_eq(&' ').is_some() {
            count += 1;
        }
        let Some(owner) = self.zero_advance.native_word_owner() else {
            writes.extend(std::iter::repeat_n(FieldWrite::UnprojectedBlank, count));
            return;
        };
        let blank = DeferredSourceBlank {
            owner,
            write: writes.len(),
            font: self.font,
        };
        writes.push(FieldWrite::DeferredBlanks {
            source: blank,
            count,
        });
        // Keep the source's typed wrapper but leave the cell invisible
        // until term_field() confirms its advance (term.c:389-427).
        flush_segment(self.output, self.buffer, self.font, self.link);
        let marker = Inline::anchor(blank.marker());
        let mut node = styled_link(String::new(), super::Font::Regular, self.link);
        if let Inline::Link { children, .. } = &mut node {
            children[0] = marker;
        } else {
            node = marker;
        }
        self.output.push(node);
    }
}
