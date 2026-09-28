//! Container events re-enter the single driver and preserve font/flush lifetimes.
use super::{InlineBuilder, Node, source_span, targets};
use crate::mandoc::containers::{ContainerSink, Event};

fn append_generated_event(builder: &mut InlineBuilder, event: Event<'_>) {
    match event {
        Event::Glyph(value) => builder.append_text(&value),
        Event::Anchor(target, source) => {
            builder.append(vec![mant_ir::Inline::anchor_at(target, source)]);
        }
        Event::Tight => builder.tighten_next_boundary(),
        Event::Release => builder.release_next_boundary(),
        Event::EmptyWord => builder.execute_empty_word(),
        _ => unreachable!("container children and font scopes handled above"),
    }
}

impl super::BlockLowerer<'_, '_> {
    fn push_no_fill_generated(
        &mut self,
        source: &Node,
        source_continuation_fallback: bool,
        finishes_row: bool,
        starts_line: bool,
        append: impl FnOnce(&mut InlineBuilder),
    ) {
        self.resume_no_fill_row();
        let armed = self.state.take_zero_advance_armed();
        let spacing = self.state.spacing_enabled();
        self.state
            .formatter
            .no_fill_inline
            .inherit_zero_advance_armed(armed);
        let formatter = &mut self.state.formatter;
        let execution = &mut formatter.execution;
        let (nodes, continues_line) = crate::mandoc::inline::lower_no_fill_fragment_with_font_state(
            spacing,
            crate::mandoc::inline::NoFillRegisters {
                font: &mut execution.font,
                row: &mut formatter.no_fill_inline,
                keep: &mut execution.keep,
            },
            &self.context.scope_posts,
            source_continuation_fallback,
            finishes_row,
            append,
        );
        let occupies_row = mant_ir::has_printable_character(&nodes);
        if occupies_row {
            self.state.execute_formatter_word();
        }
        self.state.push_preformatted(
            nodes,
            source_span(source),
            continues_line,
            starts_line,
            occupies_row,
        );
    }

    fn push_function_argument(&mut self, argument: &Node, comma_after: bool) {
        if self.state.formatter.no_fill {
            self.push_no_fill_generated(
                argument,
                super::ends_with_line_continuation(argument),
                false,
                argument.flags.line_start,
                |builder| {
                    crate::mandoc::inline::function_argument(
                        builder,
                        argument,
                        comma_after,
                        self.context.default_name,
                    );
                },
            );
        } else {
            self.state.flush_preformatted();
            self.state.push_source_inline_with(
                source_span(argument),
                false,
                false,
                false,
                |builder| {
                    crate::mandoc::inline::function_argument(
                        builder,
                        argument,
                        comma_after,
                        self.context.default_name,
                    );
                },
            );
        }
    }

    pub(super) fn push_generated_container_event(&mut self, source: &Node, event: Event<'_>) {
        if self.state.formatter.no_fill {
            // Generated words are their own term_word() calls. They consume
            // the preceding authored `\c`; only state-only events inherit it.
            let continuation = self.state.formatter.no_fill_inline.continues_source_line();
            self.push_no_fill_generated(source, continuation, false, false, |builder| {
                append_generated_event(builder, event);
            });
        } else {
            self.state.flush_preformatted();
            self.state
                .push_inline_with(source_span(source), false, false, |builder| {
                    append_generated_event(builder, event);
                });
        }
    }
}

impl super::BlockLowerer<'_, '_> {
    pub(super) fn push_container(&mut self, node: &Node) -> bool {
        if !crate::mandoc::containers::is_container(node) {
            return false;
        }
        if node.scope_end.is_none()
            && !matches!(node.macro_name.as_deref(), Some("Bf" | "Bk"))
            && !self.state.formatter.no_fill
            && !self.context.scope_posts.has_structural_payload(node)
        {
            return false;
        }
        let posts = self.context.scope_posts.clone();
        let mut sink = BlockContainerSink {
            lowerer: self,
            root: node,
            emission_source: node,
            started: false,
        };
        crate::mandoc::containers::drive(node, &posts, &mut sink)
    }
}

struct BlockContainerSink<'l, 'a, 'source> {
    lowerer: &'l mut super::BlockLowerer<'a, 'source>,
    root: &'l Node,
    emission_source: &'l Node,
    started: bool,
}

impl<'node> ContainerSink<'node> for BlockContainerSink<'node, '_, '_> {
    fn font(&mut self) -> &mut crate::mandoc::inline::FontState {
        &mut self.lowerer.state.formatter.font
    }

    fn source_node(&mut self, source: &'node Node, starts_line: bool) {
        if !self.started {
            self.lowerer.state.queue_targets(
                targets::structural_targets(self.root),
                source_span(self.root),
            );
            self.started = true;
        }
        self.emission_source = source;
        self.lowerer.observe_source_fill_mode(source);
        if self.lowerer.state.formatter.no_fill {
            self.lowerer.resume_no_fill_row();
        }
        // CVS mdoc_term.c::print_mdoc_node() settles a physical no-fill row
        // only on the next NODE_LINE (unless \c continues it).  Generated
        // posts and a preceding \z glyph therefore share the same row.
        if starts_line
            && source.flags.line_start
            && self.lowerer.state.formatter.no_fill
            && !self
                .lowerer
                .state
                .formatter
                .no_fill_inline
                .continues_source_line()
        {
            self.lowerer.settle_no_fill_inline();
            self.lowerer.state.begin_no_fill_source_line();
        }
    }

    fn event(&mut self, event: Event<'node>) {
        match event {
            // In filled structural flow input-line wrappers alone are
            // not paragraph breaks. Literal DisplayFlow consumes them.
            Event::BeginNode(_) => {}
            Event::Break => {
                self.lowerer.state.flush_preformatted();
                self.lowerer.state.flush_paragraph();
                self.lowerer.state.consume_hanging_first_line();
            }
            Event::FlushLine => self
                .lowerer
                .state
                .flush_requested_line(source_span(self.root)),
            Event::Children(nodes) => self.lowerer.push_nodes(nodes),
            Event::EnterKeep => self.lowerer.state.formatter.enter_keep_words(),
            Event::ExitKeep => self.lowerer.state.formatter.exit_keep_words(),
            Event::FunctionArgument(argument, comma_after) => {
                self.lowerer.push_function_argument(argument, comma_after);
            }
            event => self
                .lowerer
                .push_generated_container_event(self.emission_source, event),
        }
    }

    fn restore_fill(&mut self, fill: bool) {
        self.lowerer.state.formatter.no_fill = fill;
    }
}
