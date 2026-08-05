//! Couples vertical-scrollbar drawing and pointer interaction geometry.
//!
//! Ratatui's stock widget does not expose its calculated thumb rectangle.
//! Recomputing an unrelated percentage in mouse handlers made the thumb jump
//! under the pointer. This small model is therefore the single source of truth
//! for both rendering and dragging.

use ratatui::{Frame, layout::Rect};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarDrag {
    thumb_offset: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerticalScrollbar {
    area: Rect,
    content_length: usize,
    viewport_length: usize,
    position: usize,
}

impl VerticalScrollbar {
    pub(crate) fn new(
        viewport: Rect,
        content_length: usize,
        viewport_length: usize,
        position: usize,
    ) -> Option<Self> {
        if viewport.width == 0 || viewport.height == 0 || content_length <= viewport_length {
            return None;
        }
        let area = Rect::new(
            viewport.right().saturating_sub(1),
            viewport.y,
            1,
            viewport.height,
        );
        let maximum = content_length.saturating_sub(viewport_length);
        Some(Self {
            area,
            content_length,
            viewport_length,
            position: position.min(maximum),
        })
    }

    #[cfg(test)]
    pub(crate) const fn area(self) -> Rect {
        self.area
    }

    pub(crate) const fn maximum(self) -> usize {
        self.content_length.saturating_sub(self.viewport_length)
    }

    pub(crate) fn contains(self, column: u16, row: u16) -> bool {
        self.area.contains((column, row).into())
    }

    pub(crate) fn begin_drag(self, row: u16) -> (ScrollbarDrag, usize) {
        let pointer = self.local_row(row);
        let (thumb_start, thumb_length) = self.thumb_geometry();
        if pointer >= thumb_start && pointer < thumb_start.saturating_add(thumb_length) {
            return (
                ScrollbarDrag {
                    thumb_offset: pointer.saturating_sub(thumb_start),
                },
                self.position,
            );
        }

        let drag = ScrollbarDrag {
            thumb_offset: thumb_length / 2,
        };
        let position = self.position_for_pointer(row, drag);
        (drag, position)
    }

    pub(crate) fn position_for_pointer(self, row: u16, drag: ScrollbarDrag) -> usize {
        let (_, thumb_length) = self.thumb_geometry();
        let travel = usize::from(self.area.height).saturating_sub(usize::from(thumb_length));
        if travel == 0 {
            return 0;
        }
        let requested_start = usize::from(
            self.local_row(row)
                .saturating_sub(drag.thumb_offset)
                .min(self.area.height.saturating_sub(thumb_length)),
        );
        rounding_divide(self.maximum().saturating_mul(requested_start), travel).min(self.maximum())
    }

    pub(crate) fn render(self, frame: &mut Frame<'_>) {
        let (thumb_start, thumb_length) = self.thumb_geometry();
        let first_row = self.area.y.saturating_add(thumb_start);
        for offset in 0..thumb_length {
            if let Some(cell) = frame
                .buffer_mut()
                .cell_mut((self.area.x, first_row.saturating_add(offset)))
            {
                cell.set_symbol("█").set_fg(theme::OVERLAY);
            }
        }
    }

    fn local_row(self, row: u16) -> u16 {
        row.saturating_sub(self.area.y)
            .min(self.area.height.saturating_sub(1))
    }

    fn thumb_geometry(self) -> (u16, u16) {
        let track = usize::from(self.area.height);
        let thumb_length = rounding_divide(
            self.viewport_length.saturating_mul(track),
            self.content_length,
        )
        .clamp(1, track);
        let travel = track.saturating_sub(thumb_length);
        let thumb_start = if self.maximum() == 0 {
            0
        } else {
            rounding_divide(self.position.saturating_mul(travel), self.maximum()).min(travel)
        };
        (
            u16::try_from(thumb_start).unwrap_or(self.area.height),
            u16::try_from(thumb_length).unwrap_or(self.area.height),
        )
    }
}

const fn rounding_divide(numerator: usize, denominator: usize) -> usize {
    numerator.saturating_add(denominator / 2) / denominator
}

#[cfg(test)]
mod tests {
    use super::VerticalScrollbar;
    use ratatui::layout::Rect;

    #[test]
    fn thumb_reaches_both_ends_of_the_track() {
        let viewport = Rect::new(5, 7, 40, 10);
        let top = VerticalScrollbar::new(viewport, 100, 10, 0).expect("scrollbar");
        let bottom = VerticalScrollbar::new(viewport, 100, 10, 90).expect("scrollbar");

        assert_eq!(top.thumb_geometry(), (0, 1));
        assert_eq!(bottom.thumb_geometry(), (9, 1));
    }

    #[test]
    fn dragging_preserves_the_grabbed_position_inside_the_thumb() {
        let viewport = Rect::new(5, 7, 40, 20);
        let scrollbar = VerticalScrollbar::new(viewport, 100, 20, 40).expect("scrollbar");
        let (start, length) = scrollbar.thumb_geometry();
        let pointer = viewport.y + start + length - 1;
        let (drag, unchanged) = scrollbar.begin_drag(pointer);

        assert_eq!(unchanged, 40);
        assert_eq!(scrollbar.position_for_pointer(pointer, drag), 40);
        assert_eq!(scrollbar.position_for_pointer(viewport.y, drag), 0);
        assert_eq!(scrollbar.position_for_pointer(viewport.bottom(), drag), 80);
    }

    #[test]
    fn clicking_the_track_centres_the_thumb_on_the_pointer() {
        let viewport = Rect::new(5, 7, 40, 20);
        let scrollbar = VerticalScrollbar::new(viewport, 100, 20, 0).expect("scrollbar");
        let (drag, position) = scrollbar.begin_drag(viewport.y + 10);

        assert_eq!(
            position,
            scrollbar.position_for_pointer(viewport.y + 10, drag)
        );
        assert!(position > 0);
        assert!(position < scrollbar.maximum());
    }
}
