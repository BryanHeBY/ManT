//! Owns pointer gestures, selection and their page-local timer obligations.
//! Geometry and scrolling remain in App: this state never guesses hit regions.

use std::time::Instant;

use super::{SELECTION_AUTO_SCROLL_INTERVAL, SIDEBAR_RESIZE_FRAME_INTERVAL};
use crate::{RenderedSelection, TextPosition, scrollbar::ScrollbarDrag};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum PointerDrag {
    #[default]
    None,
    Sidebar,
    NavigationScrollbar(ScrollbarDrag),
    ContentScrollbar(ScrollbarDrag),
    ContentSelection {
        moved: bool,
    },
    FinderScrollbar(ScrollbarDrag),
}

#[derive(Debug, Clone, Copy)]
struct PendingSidebarResize {
    column: u16,
    deadline: Instant,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SelectionAutoScroll {
    pub(super) direction: isize,
    pub(super) column: u16,
    pub(super) deadline: Instant,
}

/// Coalesces splitter events without trailing-only debounce.
#[derive(Debug, Default)]
struct SidebarResizeSchedule {
    pending: Option<PendingSidebarResize>,
    has_live_frame: bool,
}

impl SidebarResizeSchedule {
    fn begin(&mut self) {
        self.pending = None;
        self.has_live_frame = false;
    }

    fn request(&mut self, column: u16, now: Instant) -> Option<u16> {
        if !self.has_live_frame {
            self.has_live_frame = true;
            return Some(column);
        }
        if let Some(pending) = &mut self.pending {
            // Keep the first deadline even when expensive frames receive
            // repeated coordinates before the timer can run.
            pending.column = column;
        } else {
            self.pending = Some(PendingSidebarResize {
                column,
                deadline: now + SIDEBAR_RESIZE_FRAME_INTERVAL,
            });
        }
        None
    }

    fn take_due(&mut self, now: Instant) -> Option<u16> {
        let pending = self.pending.filter(|pending| pending.deadline <= now)?;
        self.pending = None;
        Some(pending.column)
    }

    fn cancel(&mut self) {
        self.pending = None;
        self.has_live_frame = false;
    }

    fn deadline(&self) -> Option<Instant> {
        self.pending.map(|pending| pending.deadline)
    }
}

#[derive(Debug, Default)]
struct SelectionState {
    range: Option<RenderedSelection>,
    auto_scroll: Option<SelectionAutoScroll>,
}

#[derive(Debug, Default)]
pub(super) struct PointerState {
    drag: PointerDrag,
    selection: SelectionState,
    resize: SidebarResizeSchedule,
}

impl PointerState {
    pub(super) fn drag(&self) -> PointerDrag {
        self.drag
    }
    pub(super) fn selection(&self) -> Option<RenderedSelection> {
        self.selection.range
    }

    /// A new gesture replaces the old gesture's pending timers, not its
    /// retained selection. Selection survives copying and unrelated clicks.
    pub(super) fn start_drag(&mut self, drag: PointerDrag) {
        self.finish_drag();
        self.drag = drag;
        if drag == PointerDrag::Sidebar {
            self.resize.begin();
        }
    }

    pub(super) fn finish_drag(&mut self) {
        self.drag = PointerDrag::None;
        self.resize.cancel();
        self.stop_selection_scroll();
    }

    pub(super) fn page_changed(&mut self) {
        self.finish_drag();
        self.selection.range = None;
    }

    /// A changed cell map invalidates selection coordinates, but a splitter
    /// gesture may remain live while it is causing that reflow.
    pub(super) fn clear_selection(&mut self) -> bool {
        let had_selection = self.selection.range.take().is_some();
        self.stop_selection_scroll();
        if matches!(self.drag, PointerDrag::ContentSelection { .. }) {
            self.drag = PointerDrag::None;
        }
        had_selection
    }

    pub(super) fn begin_selection(&mut self, position: TextPosition, extend: bool) {
        let anchor = if extend {
            self.selection.range.map(|range| range.anchor)
        } else {
            None
        };
        self.start_drag(PointerDrag::ContentSelection {
            moved: extend && anchor.is_some(),
        });
        self.selection.range = Some(RenderedSelection {
            focus: position,
            ..RenderedSelection::new(anchor.unwrap_or(position))
        });
    }

    pub(super) fn focus_selection(&mut self, position: TextPosition) {
        if let Some(selection) = &mut self.selection.range {
            selection.focus = position;
        }
    }

    pub(super) fn update_selection_motion(&mut self, already_moved: bool) {
        if let Some(selection) = self.selection.range {
            self.drag = PointerDrag::ContentSelection {
                moved: already_moved || selection.focus != selection.anchor,
            };
        }
    }

    pub(super) fn stop_selection_scroll(&mut self) {
        self.selection.auto_scroll = None;
    }
    pub(super) fn selection_scroll(&self) -> Option<SelectionAutoScroll> {
        self.selection.auto_scroll
    }

    pub(super) fn schedule_selection_scroll(&mut self, next: Option<(isize, u16)>, now: Instant) {
        self.selection.auto_scroll = next.map(|(direction, column)| SelectionAutoScroll {
            direction,
            column,
            deadline: now + SELECTION_AUTO_SCROLL_INTERVAL,
        });
    }

    pub(super) fn request_resize(&mut self, column: u16, now: Instant) -> Option<u16> {
        self.resize.request(column, now)
    }
    pub(super) fn take_resize_due(&mut self, now: Instant) -> Option<u16> {
        self.resize.take_due(now)
    }
    pub(super) fn finish_resize(&mut self, column: u16) -> u16 {
        self.finish_drag();
        column
    }
    pub(super) fn resize_deadline(&self) -> Option<Instant> {
        self.resize.deadline()
    }

    #[cfg(test)]
    pub(super) fn pending_resize_column(&self) -> Option<u16> {
        self.resize.pending.map(|pending| pending.column)
    }
    #[cfg(test)]
    pub(super) fn restore_selection(&mut self, selection: RenderedSelection) {
        self.selection.range = Some(selection);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn gesture_replacement_cancels_old_timers_but_retains_the_selection_anchor() {
        let now = Instant::now();
        let mut pointer = PointerState::default();
        let anchor = TextPosition { row: 2, column: 3 };
        let focus = TextPosition { row: 4, column: 5 };
        pointer.begin_selection(anchor, false);
        pointer.focus_selection(focus);
        pointer.update_selection_motion(false);
        pointer.schedule_selection_scroll(Some((1, 5)), now);
        pointer.start_drag(PointerDrag::Sidebar);
        assert!(pointer.selection_scroll().is_none());
        assert_eq!(pointer.selection().unwrap().anchor, anchor);
        assert_eq!(pointer.request_resize(40, now), Some(40));
        assert_eq!(pointer.request_resize(44, now), None);
        assert!(pointer.resize_deadline().is_some());
        pointer.begin_selection(focus, true);
        assert_eq!(pointer.selection().unwrap().anchor, anchor);
        assert_eq!(pointer.selection().unwrap().focus, focus);
        assert!(pointer.resize_deadline().is_none());
        assert!(
            pointer
                .take_resize_due(now + Duration::from_secs(1))
                .is_none()
        );
        pointer.schedule_selection_scroll(Some((-1, 5)), now);
        assert!(pointer.clear_selection());
        assert_eq!(pointer.drag(), PointerDrag::None);
        assert!(pointer.selection_scroll().is_none());
        assert!(!pointer.clear_selection());
        pointer.page_changed();
        pointer.page_changed();
        assert!(pointer.selection().is_none());
    }
}
