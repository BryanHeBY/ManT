//! Observe the accepted row tail without reinterpreting its native execution.

use super::{CompletedRowOrigin, Inline, OutputRowEnd, projection::has_rendered_formatter_glyph};

const COMPLETED_ROW: &str = "\0mant:output-scope:completed-row";
const HARD_ROW: &str = "\0mant:output-scope:hard-row";
const LITERAL_ROW: &str = "\0mant:output-scope:literal-row";
const DEVICE_ROW_END: &str = "\0mant:output-scope:device-row-end";

#[derive(Clone, Copy)]
pub(super) enum RowReceipt {
    Completed(CompletedRowOrigin),
    DeviceEnd,
}

impl RowReceipt {
    pub(super) fn anchor(self) -> Inline {
        Inline::anchor(match self {
            Self::Completed(CompletedRowOrigin::Layout) => COMPLETED_ROW,
            Self::Completed(CompletedRowOrigin::HardLine) => HARD_ROW,
            Self::Completed(CompletedRowOrigin::LiteralText) => LITERAL_ROW,
            Self::DeviceEnd => DEVICE_ROW_END,
        })
    }

    fn of(node: &Inline) -> Option<Self> {
        let Inline::Anchor { id, .. } = node else {
            return None;
        };
        Some(match id.as_str() {
            COMPLETED_ROW => Self::Completed(CompletedRowOrigin::Layout),
            HARD_ROW => Self::Completed(CompletedRowOrigin::HardLine),
            LITERAL_ROW => Self::Completed(CompletedRowOrigin::LiteralText),
            DEVICE_ROW_END => Self::DeviceEnd,
            _ => return None,
        })
    }
}

/// Walk the accepted suffix backwards from its last node.
/// Wrappers are visited once and traversed only if the observer has not
/// finished. The observer decides which leaf can stop its own scan.
pub(in crate::mandoc::inline::flow) fn find_in_row_tail<T>(
    nodes: &[Inline],
    observe: &mut impl FnMut(&Inline) -> Option<T>,
) -> Option<T> {
    for node in nodes.iter().rev() {
        if let Some(result) = observe(node) {
            return Some(result);
        }
        if let Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } = node
            && let Some(result) = find_in_row_tail(children, observe)
        {
            return Some(result);
        }
    }
    None
}

/// A row-end receipt can precede or follow the same projected delimiter.
/// Accepted later cells start another row. An empty TEXT still witnesses an
/// occupied formatter word, unlike an empty wrapper or an anchor.
pub(super) fn trailing_device_row_end_receipt(nodes: &[Inline]) -> OutputRowEnd {
    find_in_row_tail(nodes, &mut |node| match RowReceipt::of(node) {
        Some(RowReceipt::DeviceEnd) => Some(OutputRowEnd::GeneratedClose),
        Some(RowReceipt::Completed(CompletedRowOrigin::LiteralText)) => {
            Some(OutputRowEnd::LiteralClose)
        }
        Some(RowReceipt::Completed(_)) => Some(OutputRowEnd::CompletedClose),
        None => match node {
            Inline::Text { .. } | Inline::Code { .. } | Inline::Equation { .. } => {
                Some(OutputRowEnd::Open)
            }
            _ => None,
        },
    })
    .unwrap_or(OutputRowEnd::Open)
}

/// Receipts are read only after native acceptance has removed rejected
/// glyphs. A literal-origin receipt retires earlier paragraph rows, while
/// a literal drain keeps that receipt in order with other completed rows.
pub(super) fn trailing_paragraph_row_origins(nodes: &[Inline]) -> Vec<CompletedRowOrigin> {
    trailing_row_origins(nodes, RowProjection::Paragraph)
}

pub(in crate::mandoc) fn trailing_completed_row_origins(
    nodes: &[Inline],
) -> Vec<CompletedRowOrigin> {
    trailing_row_origins(nodes, RowProjection::Literal)
}

#[derive(Clone, Copy)]
enum RowProjection {
    Paragraph,
    Literal,
}

fn trailing_row_origins(nodes: &[Inline], projection: RowProjection) -> Vec<CompletedRowOrigin> {
    let mut rows = Vec::new();
    let _ = find_in_row_tail(nodes, &mut |node| {
        if let Some(RowReceipt::Completed(origin)) = RowReceipt::of(node) {
            if matches!(projection, RowProjection::Paragraph)
                && origin == CompletedRowOrigin::LiteralText
            {
                return Some(());
            }
            rows.push(origin);
        }
        let (Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. }) =
            node
        else {
            return None;
        };
        let stops = match projection {
            RowProjection::Paragraph => value.chars().any(|character| !character.is_whitespace()),
            RowProjection::Literal => !value.is_empty(),
        };
        stops.then_some(())
    });
    rows.reverse();
    rows
}

/// An empty Text is an occupied row witness and deliberately stops the scan.
pub(in crate::mandoc) fn ends_with_executed_line_break(nodes: &[Inline]) -> bool {
    find_in_row_tail(nodes, &mut |node| match node {
        Inline::LineBreak { .. } => Some(true),
        Inline::Text { .. } | Inline::Code { .. } | Inline::Equation { .. } => Some(false),
        _ => None,
    }) == Some(true)
}

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
        let _ = find_in_row_tail(nodes, &mut |node| tail.observe(node));
        tail
    }

    fn observe(&mut self, node: &Inline) -> Option<()> {
        #[cfg(test)]
        {
            self.visits += 1;
        }
        match node {
            Inline::LineBreak {} => {
                self.delimiters = self.delimiters.saturating_add(1);
            }
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. }
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
        self.visible.then_some(())
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
