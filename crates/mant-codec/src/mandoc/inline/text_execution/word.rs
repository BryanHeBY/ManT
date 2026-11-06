use super::{
    Inline, RoffInlineEvent, TextEventState, TextExecutionContext, WordExecution, WordOutput,
    ZeroAdvanceState, append_empty_destination, append_text_event, flush_segment,
};

impl WordExecution {
    pub(super) fn new(context: &TextExecutionContext<'_>) -> Self {
        Self {
            output: Vec::new(),
            buffer: String::new(),
            font: context.font.display_current(),
            link: None,
            source_continuation: None,
            text_state: TextEventState::new(context.pending_word_end_break),
            native_writes: Vec::new(),
            table_device_buffer_written: false,
            table_line_needs_width: false,
        }
    }

    pub(super) fn append_source_event(
        &mut self,
        event: &RoffInlineEvent,
        context: &mut TextExecutionContext<'_>,
        is_last: bool,
    ) {
        match event {
            RoffInlineEvent::NonBreakingWordBlank => {
                self.text_state.suppress_break_whitespace = false;
                self.text_state.note_graph();
                self.text_state.graph_count += 1;
                if !self.text_state.wiped {
                    // encode(ASCII_NBRSP) buffers a non-graph source byte
                    // without consuming BACKAFTER/BACKBEFORE. Its later
                    // term_fill scan is graph and normalizes it to SP.
                    context.zero_advance.append_text(
                        " ",
                        &mut self.output,
                        &mut self.buffer,
                        self.font,
                        self.link.as_deref(),
                    );
                }
            }
            RoffInlineEvent::Text(value) => append_text_event(
                value,
                WordOutput {
                    output: &mut self.output,
                    buffer: &mut self.buffer,
                    font: self.font,
                    link: self.link.as_deref(),
                    zero_advance: context.zero_advance,
                },
                &mut self.text_state,
                context.policy.field_authoritative,
            ),
            RoffInlineEvent::Glyph(value)
            | RoffInlineEvent::Overstrike {
                terminal: Some(value),
                ..
            } => {
                self.append_glyph(value, context.zero_advance);
            }
            RoffInlineEvent::BreakableHyphen => self.append_glyph("-", context.zero_advance),
            RoffInlineEvent::FallbackGlyph(value) => {
                self.append_fallback_glyph(value, context.zero_advance);
            }
            RoffInlineEvent::DeviceName => self.append_device_name(context.zero_advance),
            RoffInlineEvent::ZeroAdvance => context.zero_advance.arm(),
            RoffInlineEvent::NoSpace => {
                let canceled_armed = context.zero_advance.cancel_armed_for_no_space();
                if is_last {
                    self.source_continuation = Some(!canceled_armed);
                }
            }
            RoffInlineEvent::Font(next_font) => {
                self.flush_segment();
                context.font.select(*next_font);
                self.font = context.font.display_current();
            }
            RoffInlineEvent::PreviousFont => {
                self.flush_segment();
                context.font.restore();
                self.font = context.font.display_current();
            }
            RoffInlineEvent::Link(target) => {
                self.flush_segment();
                self.link.clone_from(target);
            }
            RoffInlineEvent::EmptyDestination => {
                if !self.text_state.wiped {
                    append_empty_destination(
                        &mut self.output,
                        &mut self.buffer,
                        self.font,
                        self.link.as_deref(),
                        context.policy.recognize_generated_references,
                        &mut self.text_state,
                    );
                }
            }
            RoffInlineEvent::LineBreak => {
                self.text_state.pending_word_end_break = true;
                self.text_state.break_started_after_blank =
                    self.text_state.last_breakable_blank && self.text_state.graph_seen;
                self.text_state.break_trailing_blanks = self.text_state.trailing_breakable_blanks;
                self.text_state.graph_since_break = false;
            }
            RoffInlineEvent::ZeroWidthGlyph => {
                self.text_state.note_graph();
                self.text_state.zero_graph_seen = true;
            }
            RoffInlineEvent::Breakpoint => self.append_breakpoint(context),
            RoffInlineEvent::HorizontalMotion {
                reading_blank,
                argument,
            } => {
                self.append_horizontal_motion(*reading_blank, argument.as_deref(), context);
            }
            RoffInlineEvent::Presentation { .. }
            | RoffInlineEvent::Overstrike { terminal: None, .. } => {}
        }
    }

    fn append_horizontal_motion(
        &mut self,
        reading_blank: bool,
        argument: Option<&str>,
        context: &mut TextExecutionContext<'_>,
    ) {
        // ESCAPE_HORIZ clears BACKAFTER before parsing the operand. Omit
        // only this generated approximation; never remove an author blank.
        if context.zero_advance.take_armed() || !reading_blank {
            return;
        }
        let pending = context.zero_advance.has_pending_glyph();
        if pending {
            // The first positive HORIZ cell clears BACKBEFORE, rather than
            // overstriking its graph. Absolute zero motion is not this path.
            context.zero_advance.settle_horizontal_motion(
                &mut self.output,
                &mut self.buffer,
                self.font,
                self.link.as_deref(),
            );
        }
        if !pending
            || argument.is_some_and(
                libmandoc_rs::TableColumnMetrics::horizontal_motion_has_input_after_backbefore,
            )
        {
            self.append_glyph(" ", context.zero_advance);
        }
    }

    fn append_breakpoint(&mut self, context: &mut TextExecutionContext<'_>) {
        // term_fill() treats ASCII_BREAK like a zero-width breakable blank,
        // not ASCII_NBRZW: it clears graph and can stop a pending \p pass.
        // An isolated tbl word uses the same local projection rule; live
        // paragraph/definition fields keep acceptance with their cell sink.
        if self.text_state.pending_word_end_break {
            context.zero_advance.commit_pending_projection(
                &mut self.output,
                &mut self.buffer,
                self.font,
                self.link.as_deref(),
            );
            let state = &mut self.text_state;
            if !context.policy.field_authoritative {
                if state.graph_since_blank || state.graph_seen {
                    self.output.push(Inline::line_break());
                }
                state.wiped |= !state.graph_since_blank;
            }
            state.pending_word_end_break = false;
            state.graph_seen = false;
            state.graph_since_break = false;
            state.break_started_after_blank = false;
            state.break_trailing_blanks = 0;
        }
        self.text_state.graph_since_blank = false;
        self.text_state.last_breakable_blank = false;
        self.text_state.trailing_breakable_blanks = 0;
    }

    fn append_glyph(&mut self, value: &str, zero_advance: &mut ZeroAdvanceState) {
        self.text_state.suppress_break_whitespace = false;
        self.text_state.note_graph();
        self.text_state.graph_count += 1;
        if !self.text_state.wiped {
            zero_advance.append_glyph(
                value,
                &mut self.output,
                &mut self.buffer,
                self.font,
                self.link.as_deref(),
            );
        }
    }

    fn append_fallback_glyph(&mut self, value: &str, zero_advance: &mut ZeroAdvanceState) {
        if !self.text_state.wiped {
            zero_advance.append_fallback_glyph(
                value,
                &mut self.buffer,
                self.font,
                self.link.as_deref(),
            );
        }
        // Native recovery is graph even when \z erases its source spelling.
        self.text_state.suppress_break_whitespace = false;
        self.text_state.note_graph();
        self.text_state.graph_count += 1;
    }

    fn append_device_name(&mut self, zero_advance: &mut ZeroAdvanceState) {
        self.text_state.suppress_break_whitespace = false;
        self.text_state.note_graph();
        if !self.text_state.wiped {
            zero_advance.append_text(
                "utf8",
                &mut self.output,
                &mut self.buffer,
                self.font,
                self.link.as_deref(),
            );
        }
    }

    pub(super) fn flush_segment(&mut self) {
        flush_segment(
            &mut self.output,
            &mut self.buffer,
            self.font,
            self.link.as_deref(),
        );
    }
}
