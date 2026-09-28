//! Container events re-enter the single driver and preserve font/flush lifetimes.
use super::{InlineBuilder, Node, source_span, targets};
use crate::mandoc::containers::Event;

fn append_generated_event(builder: &mut InlineBuilder, event: Event<'_>) {
    match event {
        Event::Glyph(value) => builder.append_text(&value),
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
        finishes_row: bool,
        starts_line: bool,
        append: impl FnOnce(&mut InlineBuilder),
    ) {
        self.no_fill_inline
            .inherit_zero_advance_armed(self.state.take_zero_advance_armed());
        let (nodes, continues_line) = crate::mandoc::inline::lower_no_fill_fragment_with_font_state(
            self.state.spacing_enabled(),
            &mut self.formatter.font,
            &mut self.no_fill_inline,
            &self.context.scope_posts,
            super::ends_with_line_continuation(source),
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
        // Fa bypasses push_nodes(), so observe its source mode here just as
        // mdoc_term.c::print_mdoc_node() does on node entry.
        self.formatter.no_fill = argument.flags.no_fill;
        if self.formatter.no_fill {
            self.push_no_fill_generated(argument, true, argument.flags.line_start, |builder| {
                crate::mandoc::inline::function_argument(
                    builder,
                    argument,
                    comma_after,
                    self.context.default_name,
                );
            });
        } else {
            self.state.flush_preformatted();
            self.state.push_source_inline_with(
                source_span(argument),
                false,
                false,
                false,
                |builder| {
                    self.formatter.with_inline_node(builder, |builder| {
                        crate::mandoc::inline::function_argument(
                            builder,
                            argument,
                            comma_after,
                            self.context.default_name,
                        );
                    });
                },
            );
        }
    }

    fn push_generated_container_event(&mut self, source: &Node, event: Event<'_>) {
        if self.formatter.no_fill {
            self.push_no_fill_generated(source, false, false, |builder| {
                append_generated_event(builder, event);
            });
        } else {
            self.state.flush_preformatted();
            self.state
                .push_inline_with(source_span(source), false, false, |builder| {
                    self.formatter.with_inline_node(builder, |builder| {
                        append_generated_event(builder, event);
                    });
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
            && !self.context.scope_posts.has_structural_payload(node)
        {
            return false;
        }
        let mut saved_font = None;
        let mut started = false;
        let mut emission_source = node;
        let handled = crate::mandoc::containers::walk(node, &self.context.scope_posts, |event| {
            if !started {
                self.state
                    .queue_targets(targets::structural_targets(node), source_span(node));
                started = true;
            }
            match event {
                Event::At(source, starts_line) => {
                    emission_source = source;
                    // mdoc_term.c::print_mdoc_node() applies NODE_LINE at
                    // node entry, even when pre/children/post emit no word.
                    if starts_line && source.flags.line_start && self.formatter.no_fill {
                        self.state.begin_no_fill_source_line();
                    }
                }
                // In filled structural flow input-line wrappers alone are
                // not paragraph breaks. Literal DisplayFlow consumes them.
                Event::BeginNode(part) => {
                    if part.kind == libmandoc_rs::NodeKind::Head
                        && part.macro_name.as_deref() == Some("Fo")
                        && let Some(target) = targets::raw_target(part)
                    {
                        self.state
                            .push_inline_with(source_span(part), false, false, |builder| {
                                builder.append(vec![mant_ir::Inline::anchor_at(
                                    target,
                                    source_span(part),
                                )]);
                            });
                    }
                }
                Event::Break => {
                    self.state.flush_preformatted();
                    self.state.flush_paragraph();
                    self.state.consume_hanging_first_line();
                }
                Event::FlushLine => self.state.flush_requested_line(source_span(node)),
                Event::Children(nodes) => self.push_nodes(nodes),
                Event::EnterFont(font, body_id) => {
                    let saved = self.formatter.font.push_scope(font);
                    if let Some(body_id) = body_id {
                        self.context.scope_posts.enter_font(body_id, saved);
                    } else {
                        saved_font = Some(saved);
                    }
                }
                Event::ExitFont(body_id) => {
                    let saved = body_id
                        .and_then(|body_id| self.context.scope_posts.exit_font(body_id))
                        .or_else(|| body_id.is_none().then(|| saved_font.take()).flatten());
                    if let Some(saved) = saved {
                        self.formatter.font.pop_scope(saved);
                    }
                }
                Event::EnterKeep => {
                    self.state
                        .push_inline_with(source_span(node), false, false, |builder| {
                            builder.enter_keep_words();
                        });
                }
                Event::ExitKeep => {
                    self.state
                        .push_inline_with(source_span(node), false, false, |builder| {
                            builder.exit_keep_words();
                        });
                }
                Event::FunctionArgument(argument, comma_after) => {
                    self.push_function_argument(argument, comma_after);
                }
                event => self.push_generated_container_event(emission_source, event),
            }
        });
        if !handled {
            return false;
        }
        true
    }
}
