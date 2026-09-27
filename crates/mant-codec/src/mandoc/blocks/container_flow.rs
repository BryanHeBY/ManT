//! Container events re-enter the single driver and preserve font/flush lifetimes.
use super::{Node, source_span, targets};

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
        let handled = crate::mandoc::containers::walk(node, &self.context.scope_posts, |event| {
            use crate::mandoc::containers::Event;
            if !started {
                self.state
                    .queue_targets(targets::structural_targets(node), source_span(node));
                started = true;
            }
            match event {
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
                Event::EnterFont(font) => saved_font = Some(self.formatter.font.push_scope(font)),
                Event::ExitFont => {
                    if let Some(saved) = saved_font.take() {
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
                event => self
                    .state
                    .push_inline_with(source_span(node), false, false, |builder| {
                        self.formatter
                            .with_inline_node(builder, |builder| match event {
                                Event::Glyph(value) => builder.append_text(&value),
                                Event::Tight => builder.tighten_next_boundary(),
                                Event::Release => builder.release_next_boundary(),
                                Event::EmptyWord => builder.execute_empty_word(),
                                _ => {
                                    unreachable!("container children and font scopes handled above")
                                }
                            });
                    }),
            }
        });
        if !handled {
            return false;
        }
        true
    }
}
