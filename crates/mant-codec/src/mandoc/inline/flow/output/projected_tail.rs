//! Reconcile accepted empty loops with retained reading rows.

use super::{Inline, projection::has_rendered_formatter_glyph};

/// Reconcile one accepted plain-input interval with its retained delimiters.
/// For a literal, a nonempty reading contribution occupies a row even if
/// `term_field()` printed no graph (for example a positive HORIZ at the seam).
/// Do not publish a completed loop row again when its delimiter is kept
/// with that payload. The final native endline/request is a separate fact.
#[derive(Default)]
pub(in crate::mandoc::inline::flow) struct ProjectedTail {
    delimiters: u16,
    last_payload: Option<u16>,
    visible: bool,
    #[cfg(test)]
    visits: usize,
}

impl ProjectedTail {
    pub(in crate::mandoc::inline::flow) fn of(nodes: &[Inline]) -> Self {
        let mut tail = Self::default();
        tail.visit(nodes);
        tail
    }

    fn visit(&mut self, nodes: &[Inline]) {
        for node in nodes.iter().rev() {
            #[cfg(test)]
            {
                self.visits += 1;
            }
            match node {
                Inline::LineBreak {} => {
                    self.delimiters = self.delimiters.saturating_add(1);
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => self.visit(children),
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. }
                    if !value.is_empty()
                        && super::native_passes::native_word_marker(node).is_none() =>
                {
                    self.visible = has_rendered_formatter_glyph(std::slice::from_ref(node));
                    if !self.visible {
                        self.last_payload.get_or_insert(self.delimiters);
                    }
                }
                _ => {}
            }
            if self.visible {
                return;
            }
        }
    }

    pub(in crate::mandoc::inline::flow) const fn needs_prefix_close(&self) -> bool {
        self.visible && self.delimiters == 0
    }

    pub(in crate::mandoc::inline::flow) fn empty_delimiters(&self) -> u16 {
        self.delimiters.saturating_sub(u16::from(self.visible))
    }

    pub(in crate::mandoc::inline::flow) fn literal_owned_rows(&self) -> u16 {
        self.last_payload.map_or(0, |after| {
            // Keep the delimiter closing that payload too, when present.
            self.empty_delimiters()
                .saturating_sub(after.saturating_sub(1))
        })
    }

    pub(in crate::mandoc::inline::flow) fn has_open_reading_row(&self) -> bool {
        self.last_payload == Some(0)
    }
}

#[cfg(test)]
#[test]
fn projected_tail_visits_each_boundary_and_wrapper_once() {
    // This is the projection shape of the pinned ALPHA / accepted
    // NBRZW passes / retained HORIZ cases, with a longer owned tail.
    for rows in [1, 2, 16, 128, 1024] {
        let mut nodes = vec![
            Inline::Text {
                value: "ALPHA".to_owned(),
            },
            Inline::line_break(),
        ];
        for row in 0..rows {
            let children = vec![Inline::line_break()];
            nodes.push(if row % 2 == 0 {
                Inline::Strong { children }
            } else {
                Inline::Emphasis { children }
            });
        }
        nodes.push(Inline::Text {
            value: " ".repeat(1024),
        });
        let tail = ProjectedTail::of(&nodes);
        assert_eq!(usize::from(tail.literal_owned_rows()), rows);
        assert_eq!(tail.visits, 2 * rows + 3);
    }
}
