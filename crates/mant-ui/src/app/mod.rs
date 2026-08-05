//! Interactive state machine and Ratatui widget composition.

mod input;
mod menu;
mod navigation;
mod render;
mod search;

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use mant_ast::QueryBundle;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

use self::{menu::MenuId, search::SearchState};

use crate::{
    DocumentView, NavKind, RenderedDocument,
    layout::DEFAULT_SIDEBAR_WIDTH,
    scrollbar::{ScrollbarDrag, VerticalScrollbar},
};

const NAVIGATION_SYNC_IDLE: Duration = Duration::from_millis(140);
/// Defers expensive width-dependent document reflow until the splitter settles.
///
/// Pointer events can arrive substantially faster than a large manual can be
/// lowered into visual rows. Keeping only the newest coordinate and restarting
/// this short idle window avoids replaying every intermediate width.
const SIDEBAR_RESIZE_IDLE: Duration = Duration::from_millis(60);

/// Whether an input or timer update changed visible application state.
///
/// The terminal loop uses this result to avoid rebuilding large documents for
/// bookkeeping-only events, notably intermediate sidebar drag coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    Unchanged,
    Redraw,
}

impl UpdateOutcome {
    #[must_use]
    pub const fn needs_redraw(self) -> bool {
        matches!(self, Self::Redraw)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    Menu { id: MenuId, cursor: usize },
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerDrag {
    None,
    Sidebar,
    NavigationScrollbar(ScrollbarDrag),
    ContentScrollbar(ScrollbarDrag),
}

#[derive(Debug, Clone, Copy)]
struct PendingSidebarResize {
    column: u16,
    deadline: Instant,
}

/// Geometry retained from the previous frame for pointer hit testing.
///
/// Keeping these values together makes the boundary between layout/rendering
/// and event handling explicit: input code may inspect the last complete
/// frame, but it does not partially recompute layout on its own.
#[derive(Debug, Default)]
struct FrameGeometry {
    body: Rect,
    content: Rect,
    content_scrollbar: Option<VerticalScrollbar>,
    navigation: Rect,
    navigation_scrollbar: Option<VerticalScrollbar>,
    sidebar_splitter: Rect,
    status: Rect,
    navigation_rows: Vec<usize>,
}

/// All mutable interaction state for one `ManT` document.
pub struct App {
    document: DocumentView,
    selected: usize,
    expanded: HashSet<String>,
    content_scroll: usize,
    navigation_scroll: usize,
    navigation_visibility_target: Option<usize>,
    sidebar_width: u16,
    show_sidebar: bool,
    quit: bool,
    search: SearchState,
    overlay: Overlay,
    pointer_drag: PointerDrag,
    geometry: FrameGeometry,
    navigation_sync_deadline: Option<Instant>,
    pending_sidebar_resize: Option<PendingSidebarResize>,
    content_render_width: u16,
    rendered_cache: HashMap<u16, RenderedDocument>,
}

impl App {
    #[must_use]
    pub fn new(bundle: &QueryBundle) -> Self {
        let document = DocumentView::new(bundle);
        let expanded = document
            .navigation()
            .iter()
            .filter(|item| item.kind == NavKind::Section && item.depth == 0)
            .map(|item| item.id.clone())
            .collect();
        Self {
            document,
            selected: 0,
            expanded,
            content_scroll: 0,
            navigation_scroll: 0,
            navigation_visibility_target: Some(0),
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            show_sidebar: true,
            quit: false,
            search: SearchState::default(),
            overlay: Overlay::None,
            pointer_drag: PointerDrag::None,
            geometry: FrameGeometry::default(),
            navigation_sync_deadline: None,
            pending_sidebar_resize: None,
            content_render_width: 0,
            rendered_cache: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    pub(crate) fn tick(&mut self, now: Instant) -> UpdateOutcome {
        let mut outcome = UpdateOutcome::Unchanged;
        if let Some(pending) = self
            .pending_sidebar_resize
            .filter(|pending| pending.deadline <= now)
        {
            self.pending_sidebar_resize = None;
            self.commit_sidebar_at(pending.column);
            outcome = UpdateOutcome::Redraw;
        }
        if self
            .navigation_sync_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            self.navigation_sync_deadline = None;
            self.sync_selection_to_scroll();
            outcome = UpdateOutcome::Redraw;
        }
        outcome
    }

    pub(crate) fn next_wakeup(&self, now: Instant) -> Option<Duration> {
        [
            self.navigation_sync_deadline,
            self.pending_sidebar_resize.map(|pending| pending.deadline),
        ]
        .into_iter()
        .flatten()
        .map(|deadline| deadline.saturating_duration_since(now))
        .min()
    }
}

fn fit_to_width(value: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        used += character_width;
        result.push(character);
    }
    result.push_str(&" ".repeat(width.saturating_sub(used)));
    result
}

#[cfg(test)]
mod tests;
