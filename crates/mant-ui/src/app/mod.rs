//! Interactive state machine and Ratatui widget composition.

mod finder;
mod input;
mod menu;
mod navigation;
mod render;
mod search;

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use mant_ast::{DocumentAddress, DocumentCatalog, DocumentSummary, QueryBundle};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

use self::{finder::FinderState, menu::MenuId, search::SearchState};

use crate::{
    DocumentView, NavKind, RenderedDocument,
    layout::DEFAULT_SIDEBAR_WIDTH,
    scrollbar::{ScrollbarDrag, VerticalScrollbar},
};

const NAVIGATION_SYNC_IDLE: Duration = Duration::from_millis(140);
/// Caps expensive width-dependent document reflow while the splitter moves.
///
/// The first effective movement is rendered immediately. Further pointer
/// events are coalesced into at most one intermediate frame per interval, and
/// releasing the pointer always commits the final coordinate.
const SIDEBAR_RESIZE_FRAME_INTERVAL: Duration = Duration::from_millis(50);

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
    DocumentFinder,
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

/// Coalesces high-frequency splitter events without turning resize into a
/// trailing-only debounce.
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
            // Do not postpone the deadline: events arriving during an
            // expensive frame are collapsed into the scheduled frame.
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

    fn finish(&mut self, column: u16) -> u16 {
        self.cancel();
        column
    }

    fn cancel(&mut self) {
        self.pending = None;
        self.has_live_frame = false;
    }

    fn deadline(&self) -> Option<Instant> {
        self.pending.map(|pending| pending.deadline)
    }
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
    catalog: Vec<DocumentSummary>,
    finder: FinderState,
    pending_open: Option<DocumentAddress>,
    notice: Option<String>,
    overlay: Overlay,
    pointer_drag: PointerDrag,
    geometry: FrameGeometry,
    navigation_sync_deadline: Option<Instant>,
    sidebar_resize: SidebarResizeSchedule,
    content_render_width: u16,
    rendered_cache: HashMap<u16, RenderedDocument>,
}

impl App {
    #[must_use]
    pub fn new(bundle: &QueryBundle) -> Self {
        Self::with_catalog(
            bundle,
            DocumentCatalog {
                schema: mant_ast::CatalogSchema::V7,
                total: 0,
                returned: 0,
                offset: 0,
                truncated: false,
                next_offset: None,
                documents: Vec::new(),
            },
        )
    }

    #[must_use]
    pub fn with_catalog(bundle: &QueryBundle, catalog: DocumentCatalog) -> Self {
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
            catalog: catalog.documents,
            finder: FinderState::default(),
            pending_open: None,
            notice: None,
            overlay: Overlay::None,
            pointer_drag: PointerDrag::None,
            geometry: FrameGeometry::default(),
            navigation_sync_deadline: None,
            sidebar_resize: SidebarResizeSchedule::default(),
            content_render_width: 0,
            rendered_cache: HashMap::new(),
        }
    }

    pub(crate) fn take_open_request(&mut self) -> Option<DocumentAddress> {
        self.pending_open.take()
    }

    pub(crate) fn open_document(&mut self, bundle: &QueryBundle) {
        self.document = DocumentView::new(bundle);
        self.selected = 0;
        self.expanded = self
            .document
            .navigation()
            .iter()
            .filter(|item| item.kind == NavKind::Section && item.depth == 0)
            .map(|item| item.id.clone())
            .collect();
        self.content_scroll = 0;
        self.navigation_scroll = 0;
        self.navigation_visibility_target = Some(0);
        self.search = SearchState::default();
        self.overlay = Overlay::None;
        self.pointer_drag = PointerDrag::None;
        self.navigation_sync_deadline = None;
        self.rendered_cache.clear();
        self.content_render_width = 0;
        self.notice = None;
    }

    pub(crate) fn report_open_error(&mut self, message: String) {
        self.notice = Some(message);
    }

    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    pub(crate) fn tick(&mut self, now: Instant) -> UpdateOutcome {
        let mut outcome = UpdateOutcome::Unchanged;
        if let Some(column) = self.sidebar_resize.take_due(now)
            && self.commit_sidebar_at(column)
        {
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
            self.sidebar_resize.deadline(),
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
