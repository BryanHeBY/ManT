//! Container events re-enter the single driver and preserve font/flush lifetimes.
use super::{Node, source_span, targets};

impl super::BlockLowerer<'_, '_> {
    pub(super) fn push_container(&mut self, node: &Node) -> bool {
        if !crate::mandoc::containers::is_container(node) {
            return false;
        }
        if !matches!(node.macro_name.as_deref(), Some("Bf" | "Bk"))
            && !crate::mandoc::containers::has_structural_payload(node)
        {
            return false;
        }
        let mut saved_font = None;
        let mut started = false;
        let handled = crate::mandoc::containers::walk(node, |event| {
            use crate::mandoc::containers::Event;
            if !started {
                self.state
                    .queue_targets(targets::structural_targets(node), source_span(node));
                started = true;
            }
            match event {
                // In filled structural flow input-line wrappers alone are
                // not paragraph breaks. Literal DisplayFlow consumes them.
                Event::BeginNode(_) => {}
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
                event => self
                    .state
                    .push_inline_with(source_span(node), false, false, |builder| {
                        builder.font = self.formatter.font;
                        match event {
                            Event::Glyph(value) => builder.append_text(&value),
                            Event::Tight => builder.tighten_next_boundary(),
                            Event::Release => builder.release_next_boundary(),
                            Event::EmptyWord => builder.execute_empty_word(),
                            _ => unreachable!("container children and font scopes handled above"),
                        }
                    }),
            }
        });
        if !handled {
            return false;
        }
        true
    }
}
