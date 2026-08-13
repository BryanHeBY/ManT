//! Interactive state machine and Ratatui widget composition.

mod finder;
mod input;
mod menu;
mod navigation;
mod render;
mod search;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use mant_ir::ResolvedContent;
use mant_protocol::{CatalogQuery, DocumentAddress, DocumentCatalog};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

use self::{finder::FinderState, menu::MenuId, search::SearchState};

use crate::{
    DocumentView, NavKind, RenderedDocument,
    layout::DEFAULT_SIDEBAR_WIDTH,
    scrollbar::{ScrollbarDrag, VerticalScrollbar},
};

const NAVIGATION_SYNC_IDLE: Duration = Duration::from_millis(140);
const HISTORY_LIMIT: usize = 64;
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
    /// Only non-visible bookkeeping changed, if anything.
    Unchanged,
    /// Visible state changed and the terminal should be redrawn.
    Redraw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryDirection {
    New,
    Back,
    Forward,
}

#[derive(Debug, Clone)]
struct HistoryLocation {
    address: Option<DocumentAddress>,
    fallback: Option<Arc<ResolvedContent>>,
    target: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct NavigationRequest {
    address: DocumentAddress,
    target: Option<String>,
    direction: HistoryDirection,
}

impl NavigationRequest {
    pub(crate) const fn address(&self) -> &DocumentAddress {
        &self.address
    }
}

impl UpdateOutcome {
    /// Return whether this update requires a new frame.
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
    finder: FinderState,
    pending_discovery: Option<CatalogQuery>,
    pending_open: Option<NavigationRequest>,
    pending_external: Option<String>,
    current_address: Option<DocumentAddress>,
    fallback_bundle: Option<Arc<ResolvedContent>>,
    back_history: Vec<HistoryLocation>,
    forward_history: Vec<HistoryLocation>,
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
    /// Construct an application without preloaded discovery rows.
    #[must_use]
    pub fn new(bundle: &ResolvedContent) -> Self {
        Self::with_catalog(
            bundle,
            DocumentCatalog {
                schema: mant_protocol::CatalogSchema::V7,
                total: 0,
                returned: 0,
                offset: 0,
                truncated: false,
                next_offset: None,
                documents: Vec::new(),
            },
        )
    }

    /// Construct an application with a snapshot for the document finder.
    #[must_use]
    pub fn with_catalog(bundle: &ResolvedContent, catalog: DocumentCatalog) -> Self {
        let document = DocumentView::new(bundle);
        let mut finder = FinderState::default();
        finder.replace_catalog(catalog);
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
            finder,
            pending_discovery: None,
            pending_open: None,
            pending_external: None,
            current_address: bundle.address.clone(),
            fallback_bundle: bundle.address.is_none().then(|| Arc::new(bundle.clone())),
            back_history: Vec::new(),
            forward_history: Vec::new(),
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

    pub(crate) fn take_open_request(&mut self) -> Option<NavigationRequest> {
        self.pending_open.take()
    }

    pub(crate) fn take_external_request(&mut self) -> Option<String> {
        self.pending_external.take()
    }

    pub(crate) fn take_discovery_request(&mut self) -> Option<CatalogQuery> {
        self.pending_discovery.take()
    }

    pub(crate) fn complete_discovery(&mut self, catalog: DocumentCatalog) {
        self.finder.replace_catalog(catalog);
        self.notice = None;
    }

    pub(crate) fn report_discovery_error(&mut self, message: String) {
        self.notice = Some(message);
    }

    pub(crate) fn complete_open(&mut self, bundle: &ResolvedContent, request: NavigationRequest) {
        self.commit_history(request.direction);
        self.replace_document(bundle);
        if let Some(target) = request.target {
            self.jump_to_anchor(&target);
        }
    }

    fn replace_document(&mut self, bundle: &ResolvedContent) {
        self.document = DocumentView::new(bundle);
        self.current_address.clone_from(&bundle.address);
        self.fallback_bundle = bundle.address.is_none().then(|| Arc::new(bundle.clone()));
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

    pub(crate) fn report_notice(&mut self, message: String) {
        self.notice = Some(message);
    }

    fn current_location(&self) -> HistoryLocation {
        HistoryLocation {
            address: self.current_address.clone(),
            fallback: self.fallback_bundle.clone(),
            target: self
                .document
                .navigation()
                .get(self.selected)
                .map(|item| item.target_id.clone()),
        }
    }

    pub(super) fn request_open(&mut self, address: DocumentAddress, target: Option<String>) {
        if self.current_address.as_ref() == Some(&address) {
            let current = self.current_location();
            let moved = if let Some(target) = target {
                self.jump_to_anchor(&target)
            } else {
                self.jump_content(false);
                true
            };
            if moved {
                push_history(&mut self.back_history, current);
                self.forward_history.clear();
            }
            return;
        }
        self.pending_open = Some(NavigationRequest {
            address,
            target,
            direction: HistoryDirection::New,
        });
    }

    pub(super) fn navigate_history(&mut self, back: bool) {
        let location = if back {
            self.back_history.last()
        } else {
            self.forward_history.last()
        }
        .cloned();
        let Some(location) = location else {
            return;
        };
        let direction = if back {
            HistoryDirection::Back
        } else {
            HistoryDirection::Forward
        };
        if location.address == self.current_address {
            self.complete_local_history(location, direction);
        } else if let Some(address) = location.address.clone() {
            self.pending_open = Some(NavigationRequest {
                address,
                target: location.target,
                direction,
            });
        } else if let Some(bundle) = location.fallback.as_deref() {
            let bundle = bundle.clone();
            self.complete_local_bundle(&bundle, location.target, direction);
        }
    }

    fn complete_local_history(&mut self, location: HistoryLocation, direction: HistoryDirection) {
        self.commit_history(direction);
        if let Some(target) = location.target {
            self.jump_to_anchor(&target);
        }
    }

    fn complete_local_bundle(
        &mut self,
        bundle: &ResolvedContent,
        target: Option<String>,
        direction: HistoryDirection,
    ) {
        self.commit_history(direction);
        self.replace_document(bundle);
        if let Some(target) = target {
            self.jump_to_anchor(&target);
        }
    }

    fn commit_history(&mut self, direction: HistoryDirection) {
        let current = self.current_location();
        match direction {
            HistoryDirection::New => {
                push_history(&mut self.back_history, current);
                self.forward_history.clear();
            }
            HistoryDirection::Back => {
                self.back_history.pop();
                push_history(&mut self.forward_history, current);
            }
            HistoryDirection::Forward => {
                self.forward_history.pop();
                push_history(&mut self.back_history, current);
            }
        }
    }

    #[must_use]
    /// Return whether the terminal event loop should exit.
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

fn push_history(history: &mut Vec<HistoryLocation>, location: HistoryLocation) {
    if history.len() == HISTORY_LIMIT {
        history.remove(0);
    }
    history.push(location);
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
