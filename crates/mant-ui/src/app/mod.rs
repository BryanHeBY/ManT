//! Interactive state machine and Ratatui widget composition.

mod finder;
mod input;
mod menu;
mod navigation;
mod references;
mod render;
mod search;
mod session;
use session::{DocumentChangeReason, DocumentSession};
mod tabs;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use mant_ir::ResolvedContent;
use mant_protocol::{
    CatalogQuery, ContentSelector, DocumentAddress, DocumentCatalog, DocumentOpenTarget,
};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

use self::{finder::FinderState, menu::MenuId, search::SearchState};

use crate::{
    CopyFormat, CopyRequest, DocumentView, NavKind, RenderedDocument, RenderedSelection,
    layout::DEFAULT_SIDEBAR_WIDTH,
    scrollbar::{ScrollbarDrag, VerticalScrollbar},
};

const NAVIGATION_SYNC_IDLE: Duration = Duration::from_millis(140);
const COPY_TOAST_DURATION: Duration = Duration::from_millis(1_500);
const SELECTION_AUTO_SCROLL_INTERVAL: Duration = Duration::from_millis(50);
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
    reference: bool,
}

#[derive(Debug, Clone)]
struct DocumentTab {
    address: Option<DocumentAddress>,
    fallback: Option<Arc<ResolvedContent>>,
    label: String,
    target: Option<String>,
    reference: bool,
}

#[derive(Debug, Clone, Copy)]
struct DocumentTabHit {
    area: Rect,
    index: usize,
}

#[derive(Debug, Clone)]
struct CopyToast {
    message: String,
    deadline: Instant,
}

#[derive(Debug, Clone)]
pub(crate) struct NavigationRequest {
    pub(crate) document: DocumentOpenTarget,
    target: Option<String>,
    reference: bool,
    direction: HistoryDirection,
}

impl NavigationRequest {
    #[cfg(test)]
    pub(crate) const fn address(&self) -> &DocumentAddress {
        match &self.document {
            DocumentOpenTarget::Address { address } => address,
            DocumentOpenTarget::Manual { .. } => panic!("request has no qualified address"),
        }
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
    References,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerDrag {
    None,
    Sidebar,
    NavigationScrollbar(ScrollbarDrag),
    ContentScrollbar(ScrollbarDrag),
    ContentSelection { moved: bool },
    FinderScrollbar(ScrollbarDrag),
}

#[derive(Debug, Clone, Copy)]
struct PendingSidebarResize {
    column: u16,
    deadline: Instant,
}

#[derive(Debug, Clone, Copy)]
struct SelectionAutoScroll {
    direction: isize,
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
    document_tabs: Vec<DocumentTabHit>,
    previous_document_tabs: Rect,
    next_document_tabs: Rect,
    status: Rect,
    navigation_rows: Vec<usize>,
    finder_query: Rect,
    finder_results: Rect,
    finder_scrollbar: Option<VerticalScrollbar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavigationViewportRequest {
    Reveal { node_index: usize },
    PreserveRow { node_index: usize, row: usize },
}

impl NavigationViewportRequest {
    const fn node_index(self) -> usize {
        match self {
            Self::Reveal { node_index } | Self::PreserveRow { node_index, .. } => node_index,
        }
    }
}

/// All mutable interaction state for one `ManT` reader session.
pub struct App {
    session: DocumentSession,
    selected: usize,
    expanded: HashSet<String>,
    navigation_scroll: usize,
    navigation_viewport_request: Option<NavigationViewportRequest>,
    sidebar_width: u16,
    show_sidebar: bool,
    full_outline_labels: bool,
    quit: bool,
    search: SearchState,
    scope_documents: Vec<Arc<ResolvedContent>>,
    finder: FinderState,
    pending_discovery: Option<CatalogQuery>,
    pending_open: Option<NavigationRequest>,
    pending_external: Option<crate::ExternalUri>,
    pending_copy: Option<CopyRequest>,
    current_address: Option<DocumentAddress>,
    fallback_bundle: Option<Arc<ResolvedContent>>,
    back_history: Vec<HistoryLocation>,
    forward_history: Vec<HistoryLocation>,
    document_tabs: Vec<DocumentTab>,
    active_document_tab: usize,
    document_tab_scroll: usize,
    document_tab_visibility_target: Option<usize>,
    document_tab_view_width: u16,
    notice: Option<String>,
    copy_toast: Option<CopyToast>,
    overlay: Overlay,
    reference_chooser: Option<references::ReferenceChooser>,
    pointer_drag: PointerDrag,
    selection: Option<RenderedSelection>,
    selection_auto_scroll: Option<SelectionAutoScroll>,
    geometry: FrameGeometry,
    navigation_sync_deadline: Option<Instant>,
    sidebar_resize: SidebarResizeSchedule,
}

impl App {
    /// Construct an application without preloaded discovery rows.
    #[must_use]
    pub fn new(bundle: &ResolvedContent) -> Self {
        Self::with_catalog(bundle, DocumentCatalog::default())
    }

    /// Construct an application with a snapshot for the document finder.
    #[must_use]
    pub fn with_catalog(bundle: &ResolvedContent, catalog: DocumentCatalog) -> Self {
        Self::with_catalog_and_scope(bundle, catalog, std::slice::from_ref(bundle))
    }

    /// Construct an application whose in-document search spans a bounded,
    /// pre-resolved document scope.
    #[must_use]
    pub fn with_catalog_and_scope(
        bundle: &ResolvedContent,
        catalog: DocumentCatalog,
        scope: &[ResolvedContent],
    ) -> Self {
        let document = DocumentView::new(bundle);
        let current_bundle = Arc::new(bundle.clone());
        let mut finder = FinderState::default();
        finder.replace_catalog(catalog);
        let expanded = document
            .navigation()
            .iter()
            .filter(|item| item.kind == NavKind::Section && item.depth == 0)
            .map(|item| item.id.clone())
            .collect();
        let mut scope_documents = scope.iter().cloned().map(Arc::new).collect::<Vec<_>>();
        if !scope_documents
            .iter()
            .any(|candidate| candidate.address == bundle.address)
        {
            scope_documents.insert(0, Arc::new(bundle.clone()));
        }
        let mut app = Self {
            session: DocumentSession::new(Arc::clone(&current_bundle), document),
            selected: 0,
            expanded,
            navigation_scroll: 0,
            navigation_viewport_request: Some(NavigationViewportRequest::Reveal { node_index: 0 }),
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            show_sidebar: true,
            full_outline_labels: false,
            quit: false,
            search: SearchState::default(),
            scope_documents,
            finder,
            pending_discovery: None,
            pending_open: None,
            pending_external: None,
            pending_copy: None,
            current_address: bundle.address.clone(),
            fallback_bundle: bundle.address.is_none().then_some(current_bundle),
            back_history: Vec::new(),
            forward_history: Vec::new(),
            document_tabs: Vec::new(),
            active_document_tab: 0,
            document_tab_scroll: 0,
            document_tab_visibility_target: Some(0),
            document_tab_view_width: 0,
            notice: None,
            copy_toast: None,
            overlay: Overlay::None,
            reference_chooser: None,
            pointer_drag: PointerDrag::None,
            selection: None,
            selection_auto_scroll: None,
            geometry: FrameGeometry::default(),
            navigation_sync_deadline: None,
            sidebar_resize: SidebarResizeSchedule::default(),
        };
        app.sync_current_document_tab();
        app
    }

    pub(crate) fn take_open_request(&mut self) -> Option<NavigationRequest> {
        self.pending_open.take()
    }

    pub(crate) fn take_external_request(&mut self) -> Option<crate::ExternalUri> {
        self.pending_external.take()
    }

    pub(crate) fn take_copy_request(&mut self) -> Option<CopyRequest> {
        self.pending_copy.take()
    }

    pub(crate) fn take_discovery_request(&mut self) -> Option<CatalogQuery> {
        self.pending_discovery.take()
    }

    pub(crate) fn complete_discovery(&mut self, catalog: DocumentCatalog) {
        self.finder.replace_catalog(catalog);
        self.notice = None;
    }

    pub(crate) fn report_discovery_error(&mut self, message: String) {
        self.report_notice(message);
    }

    pub(crate) fn complete_open(&mut self, bundle: &ResolvedContent, request: NavigationRequest) {
        self.complete_loaded_navigation(
            bundle,
            request.target,
            request.reference,
            request.direction,
        );
    }

    fn complete_loaded_navigation(
        &mut self,
        bundle: &ResolvedContent,
        target: Option<String>,
        reference: bool,
        direction: HistoryDirection,
    ) {
        if !reference
            && let Some(target) = target.as_deref()
            && let Err(message) = validate_fragment(bundle, target)
        {
            self.report_open_error(message);
            return;
        }
        let candidate = DocumentView::new(bundle);
        if let Some(target) = target.as_deref()
            && ((reference && candidate.reference_target(target).is_none())
                || candidate
                    .render(self.geometry.content.width.max(1))
                    .anchor_row(target)
                    .is_none())
        {
            self.report_open_error("The destination has no available reveal location; the current document was retained".into());
            return;
        }
        self.commit_history(direction);
        self.replace_document_view(bundle, DocumentChangeReason::from(direction), candidate);
        if let Some(target) = target {
            self.reveal_anchor(&target);
        }
    }

    fn replace_document(&mut self, bundle: &ResolvedContent, reason: DocumentChangeReason) {
        self.replace_document_view(bundle, reason, DocumentView::new(bundle));
    }

    fn replace_document_view(
        &mut self,
        bundle: &ResolvedContent,
        reason: DocumentChangeReason,
        view: DocumentView,
    ) {
        self.remember_current_document_tab();
        self.session = DocumentSession::new(Arc::new(bundle.clone()), view);
        self.current_address.clone_from(&bundle.address);
        self.fallback_bundle = bundle.address.is_none().then(|| Arc::new(bundle.clone()));
        self.selected = 0;
        self.expanded = self
            .session
            .document
            .navigation()
            .iter()
            .filter(|item| item.kind == NavKind::Section && item.depth == 0)
            .map(|item| item.id.clone())
            .collect();
        self.navigation_scroll = 0;
        self.navigation_viewport_request =
            Some(NavigationViewportRequest::Reveal { node_index: 0 });
        if reason != DocumentChangeReason::SearchResult {
            self.search = SearchState::default();
        }
        self.overlay = Overlay::None;
        self.pointer_drag = PointerDrag::None;
        self.selection = None;
        self.selection_auto_scroll = None;
        self.navigation_sync_deadline = None;
        self.notice = None;
        self.copy_toast = None;
        self.sync_current_document_tab();
    }

    pub(super) fn copy_selection(&mut self) {
        let Some(selection) = self.selection else {
            self.report_notice("Drag across document text before copying".to_owned());
            return;
        };
        let Some(rendered) = self
            .session
            .rendered_cache
            .get(&self.session.content_render_width)
        else {
            self.report_notice("The document is not ready to copy".to_owned());
            return;
        };
        let text = rendered.selected_text(selection);
        if text.is_empty() {
            self.report_notice("The selected cells contain no text".to_owned());
        } else if text.len() > crate::MAX_COPY_BYTES {
            self.report_notice("The selection exceeds the 4 MiB clipboard limit".to_owned());
        } else {
            self.pending_copy = Some(CopyRequest::Selection { text });
        }
    }

    pub(super) fn copy_selected_node(&mut self, format: CopyFormat) {
        let Some(node) = self.session.document.navigation().get(self.selected) else {
            self.report_notice("No document node is selected".to_owned());
            return;
        };
        if matches!(
            node.kind,
            NavKind::EntryGroup
                | NavKind::ReferenceGroup
                | NavKind::Reference
                | NavKind::ReferenceNotice
        ) {
            self.report_notice("Select a complete document node before copying".to_owned());
            return;
        }
        self.pending_copy = Some(CopyRequest::Node {
            content: Arc::clone(&self.session.current_bundle),
            selector: if node.kind == NavKind::Tldr {
                ContentSelector::path("0")
            } else {
                ContentSelector::id(node.id.clone())
            },
            format,
        });
    }

    pub(super) fn copy_selected_reference(&mut self) {
        let reference = self
            .session
            .document
            .navigation()
            .get(self.selected)
            .filter(|node| self.session.document.reference_target(&node.id).is_some())
            .map(|node| node.id.clone());
        if let Some(id) = reference {
            self.queue_reference_copy(&id);
        } else {
            self.show_reference_chooser(true);
        }
    }

    pub(crate) fn report_open_error(&mut self, message: String) {
        self.report_notice(message);
    }

    pub(crate) fn report_notice(&mut self, message: String) {
        self.copy_toast = None;
        self.notice = Some(message);
    }

    pub(crate) fn report_copy_success(&mut self, message: String) {
        self.report_copy_success_at(message, Instant::now());
    }

    fn report_copy_success_at(&mut self, message: String, now: Instant) {
        self.notice = None;
        self.copy_toast = Some(CopyToast {
            message,
            deadline: now + COPY_TOAST_DURATION,
        });
    }

    fn current_location(&self) -> HistoryLocation {
        let target = self
            .session
            .document
            .navigation()
            .get(self.selected)
            .map(|item| item.target_id.clone());
        let reference = target
            .as_deref()
            .is_some_and(|target| self.session.document.reference_target(target).is_some());
        HistoryLocation {
            address: self.current_address.clone(),
            fallback: self.fallback_bundle.clone(),
            target,
            reference,
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
            document: address.into(),
            target,
            reference: false,
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
                document: address.into(),
                target: location.target,
                reference: location.reference,
                direction,
            });
        } else if let Some(bundle) = location.fallback.as_deref() {
            let bundle = bundle.clone();
            self.complete_local_bundle(&bundle, location.target, location.reference, direction);
        }
    }

    fn complete_local_history(&mut self, location: HistoryLocation, direction: HistoryDirection) {
        if let Some(target) = location.target.as_deref() {
            if location.reference {
                if self.session.document.reference_location(target).is_none() {
                    self.report_notice(
                        "The historical reference location is no longer available".into(),
                    );
                    return;
                }
            } else if let Err(message) = validate_fragment(&self.session.current_bundle, target) {
                self.report_notice(message);
                return;
            }
            if self
                .session
                .document
                .render(self.geometry.content.width.max(1))
                .anchor_row(target)
                .is_none()
            {
                self.report_notice("The historical target has no rendered content location".into());
                return;
            }
        }
        self.commit_history(direction);
        if let Some(target) = location.target {
            self.reveal_anchor(&target);
        }
    }

    fn complete_local_bundle(
        &mut self,
        bundle: &ResolvedContent,
        target: Option<String>,
        reference: bool,
        direction: HistoryDirection,
    ) {
        self.complete_loaded_navigation(bundle, target, reference, direction);
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
        if self
            .copy_toast
            .as_ref()
            .is_some_and(|toast| toast.deadline <= now)
        {
            self.copy_toast = None;
            outcome = UpdateOutcome::Redraw;
        }
        if self.tick_selection_auto_scroll(now) {
            outcome = UpdateOutcome::Redraw;
        }
        outcome
    }

    pub(crate) fn next_wakeup(&self, now: Instant) -> Option<Duration> {
        [
            self.navigation_sync_deadline,
            self.sidebar_resize.deadline(),
            self.copy_toast.as_ref().map(|toast| toast.deadline),
            self.selection_auto_scroll.map(|scroll| scroll.deadline),
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

/// Validate against the candidate snapshot before changing any navigation state.
/// The shared target stream merges an entry's own anchor with its item, while
/// two independent owners remain ambiguous even when their spelling is equal.
fn validate_fragment(bundle: &ResolvedContent, fragment: &str) -> Result<(), String> {
    use mant_ir::{
        NavigationEvent, NavigationScanOptions, ReferenceLinkFilter, ReferenceScanLimits,
        ReferenceScope, scan_navigation_scope,
    };
    use std::ops::ControlFlow;
    let tldr = fragment == "tldr" && bundle.tldr.is_some();
    let Some(document) = bundle.document.as_ref() else {
        return if tldr {
            Ok(())
        } else {
            Err(format!("No outline node matches #{fragment}"))
        };
    };
    let mut found: Option<mant_ir::ContentReveal> = None;
    let mut ambiguous = false;
    let mut position_limited = false;
    let report = scan_navigation_scope(
        document,
        ReferenceScope::Document,
        ReferenceScanLimits::default(),
        NavigationScanOptions {
            links: ReferenceLinkFilter::NONE,
            targets: true,
            entry_sets: false,
        },
        |event, budget| {
            if let NavigationEvent::Target(target) = event
                && (target.id.as_str() == fragment
                    || target
                        .aliases
                        .iter()
                        .any(|alias| alias.as_str() == fragment))
            {
                if tldr {
                    ambiguous = true;
                    return ControlFlow::Break(());
                }
                if budget
                    .consume(
                        target.reveal.depth(),
                        target.reveal.depth().saturating_mul(3),
                        0,
                    )
                    .is_err()
                {
                    return ControlFlow::Break(());
                }
                if let Some(previous) = found.as_ref() {
                    if previous.as_ref() != target.reveal {
                        ambiguous = true;
                        return ControlFlow::Break(());
                    }
                } else if let Some(reveal) = target.reveal.to_owned() {
                    found = Some(reveal);
                } else {
                    position_limited = true;
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        },
    );
    if ambiguous {
        Err(format!("Ambiguous local target #{fragment}"))
    } else if position_limited || !report.complete() {
        Err(format!(
            "Local target #{fragment} was not verified within the navigation budget"
        ))
    } else if found.is_none() && !tldr {
        Err(format!("No outline node matches #{fragment}"))
    } else {
        Ok(())
    }
}

fn fit_to_width(value: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0;
    for character in crate::text::sanitize_terminal_text(value).chars() {
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
