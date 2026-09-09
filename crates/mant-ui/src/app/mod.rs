//! Interactive state machine and Ratatui widget composition.

mod effects;
use effects::PendingEffects;
mod pointer;
use pointer::{PointerDrag, PointerState};
mod finder;
mod input;
mod menu;
mod navigation;
mod navigation_state;
use navigation_state::{HistoryDirection, HistoryLocation, LocalTarget, NavigationState};
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
    CopyFormat, CopyRequest, DocumentView, NavKind, RenderedDocument,
    layout::DEFAULT_SIDEBAR_WIDTH, scrollbar::VerticalScrollbar,
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
    target: LocalTarget,
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

#[derive(Debug, PartialEq, Eq)]
enum Overlay {
    None,
    Menu { id: MenuId, cursor: usize },
    DocumentFinder,
    Help,
    References(references::ReferenceChooser),
}

impl Overlay {
    fn references(&self) -> Option<&references::ReferenceChooser> {
        match self {
            Self::References(chooser) => Some(chooser),
            _ => None,
        }
    }

    fn references_mut(&mut self) -> Option<&mut references::ReferenceChooser> {
        match self {
            Self::References(chooser) => Some(chooser),
            _ => None,
        }
    }

    fn take_references(&mut self) -> Option<references::ReferenceChooser> {
        if !matches!(self, Self::References(_)) {
            return None;
        }
        match std::mem::replace(self, Self::None) {
            Self::References(chooser) => Some(chooser),
            _ => None,
        }
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

/// Immutable snapshots supplied by the host at reader startup.
///
/// Handles are shared without copying document bodies. Equal document addresses
/// do not imply equal revisions; scope membership uses allocation identity.
pub struct ReaderOptions {
    /// Initially displayed snapshot.
    pub current: Arc<ResolvedContent>,
    /// Initial read-only document-finder inventory.
    pub catalog: DocumentCatalog,
    /// Ordered, pre-resolved snapshots searched by the reader.
    pub scope: Vec<Arc<ResolvedContent>>,
}

impl ReaderOptions {
    /// Start with only the current snapshot and an empty finder inventory.
    #[must_use]
    pub fn new(current: Arc<ResolvedContent>) -> Self {
        Self {
            scope: vec![Arc::clone(&current)],
            current,
            catalog: DocumentCatalog::default(),
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
    effects: PendingEffects,
    navigation: NavigationState,
    notice: Option<String>,
    copy_toast: Option<CopyToast>,
    overlay: Overlay,
    pointer: PointerState,
    geometry: FrameGeometry,
    navigation_sync_deadline: Option<Instant>,
}

impl App {
    /// Construct an application without preloaded discovery rows.
    /// Copies the borrowed snapshot; use [`Self::from_shared`] to share it.
    #[must_use]
    pub fn new(bundle: &ResolvedContent) -> Self {
        Self::with_catalog(bundle, DocumentCatalog::default())
    }

    /// Construct an application with a snapshot for the document finder.
    /// Copies the borrowed document; use [`Self::from_shared`] to share it.
    #[must_use]
    pub fn with_catalog(bundle: &ResolvedContent, catalog: DocumentCatalog) -> Self {
        Self::with_catalog_and_scope(bundle, catalog, std::slice::from_ref(bundle))
    }

    /// Construct an application whose in-document search spans a bounded,
    /// pre-resolved document scope.
    ///
    /// Copies each borrowed scope member once, reusing its new handle when
    /// `bundle` is that exact member. Equal addresses are not merged.
    #[must_use]
    pub fn with_catalog_and_scope(
        bundle: &ResolvedContent,
        catalog: DocumentCatalog,
        scope: &[ResolvedContent],
    ) -> Self {
        let scope_documents = scope.iter().cloned().map(Arc::new).collect::<Vec<_>>();
        let current = scope
            .iter()
            .position(|candidate| std::ptr::eq(candidate, bundle))
            .map_or_else(
                || Arc::new(bundle.clone()),
                |index| Arc::clone(&scope_documents[index]),
            );
        Self::from_shared(ReaderOptions {
            current,
            catalog,
            scope: scope_documents,
        })
    }

    /// Construct a reader from host-owned immutable snapshots without copying
    /// their document bodies. If absent by pointer identity, the current
    /// snapshot is prepended to the supplied search scope.
    #[must_use]
    pub fn from_shared(options: ReaderOptions) -> Self {
        let ReaderOptions {
            current: current_bundle,
            catalog,
            scope: mut scope_documents,
        } = options;
        let document = DocumentView::new(&current_bundle);
        let mut finder = FinderState::default();
        finder.replace_catalog(catalog);
        let expanded = document
            .navigation()
            .iter()
            .filter(|item| item.kind == NavKind::Section && item.depth == 0)
            .map(|item| item.id.clone())
            .collect();
        if !scope_documents
            .iter()
            .any(|candidate| Arc::ptr_eq(candidate, &current_bundle))
        {
            scope_documents.insert(0, Arc::clone(&current_bundle));
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
            effects: PendingEffects::default(),
            navigation: NavigationState::new(
                current_bundle.address.clone(),
                current_bundle
                    .address
                    .is_none()
                    .then(|| Arc::clone(&current_bundle)),
                Arc::downgrade(&current_bundle),
            ),
            notice: None,
            copy_toast: None,
            overlay: Overlay::None,
            pointer: PointerState::default(),
            geometry: FrameGeometry::default(),
            navigation_sync_deadline: None,
        };
        app.sync_current_document_tab();
        app
    }

    pub(crate) fn take_open_request(&mut self) -> Option<NavigationRequest> {
        self.effects.take_open()
    }

    pub(crate) fn take_external_request(&mut self) -> Option<crate::ExternalUri> {
        self.effects.take_external()
    }

    pub(crate) fn take_copy_request(&mut self) -> Option<CopyRequest> {
        self.effects.take_copy()
    }

    pub(crate) fn take_discovery_request(&mut self) -> Option<CatalogQuery> {
        self.effects.take_discovery()
    }

    pub(crate) fn complete_discovery(&mut self, catalog: DocumentCatalog) {
        self.finder.replace_catalog(catalog);
        self.notice = None;
    }

    pub(crate) fn report_discovery_error(&mut self, message: String) {
        self.report_notice(message);
    }

    #[cfg(test)]
    pub(crate) fn complete_open(&mut self, bundle: &ResolvedContent, request: NavigationRequest) {
        self.complete_open_shared(Arc::new(bundle.clone()), request);
    }

    pub(crate) fn complete_open_shared(
        &mut self,
        bundle: Arc<ResolvedContent>,
        request: NavigationRequest,
    ) {
        self.complete_loaded_navigation(bundle, request.target, request.direction);
    }

    fn complete_loaded_navigation(
        &mut self,
        bundle: Arc<ResolvedContent>,
        target: LocalTarget,
        direction: HistoryDirection,
    ) {
        if let LocalTarget::Fragment(target) = &target
            && let Err(message) = validate_fragment(&bundle, target)
        {
            self.report_open_error(message);
            return;
        }
        let candidate = DocumentView::new(&bundle);
        if let Some(id) = target.id()
            && ((matches!(target, LocalTarget::ReferenceOccurrence(_))
                && candidate.reference_target(id).is_none())
                || candidate
                    .render(self.geometry.content.width.max(1))
                    .anchor_row(id)
                    .is_none())
        {
            self.report_open_error("The destination has no available reveal location; the current document was retained".into());
            return;
        }
        self.commit_history(direction);
        self.replace_document_view(bundle, DocumentChangeReason::from(direction), candidate);
        if let LocalTarget::Fragment(target) | LocalTarget::ReferenceOccurrence(target) = target {
            self.reveal_anchor(&target);
        }
    }

    fn replace_document(&mut self, bundle: Arc<ResolvedContent>, reason: DocumentChangeReason) {
        let view = DocumentView::new(&bundle);
        self.replace_document_view(bundle, reason, view);
    }

    fn replace_document_view(
        &mut self,
        bundle: Arc<ResolvedContent>,
        reason: DocumentChangeReason,
        view: DocumentView,
    ) {
        self.remember_current_document_tab();
        self.navigation.replace_current(
            bundle.address.clone(),
            bundle.address.is_none().then(|| Arc::clone(&bundle)),
            Arc::downgrade(&bundle),
        );
        self.session = DocumentSession::new(bundle, view);
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
        self.pointer.page_changed();
        self.navigation_sync_deadline = None;
        self.notice = None;
        self.copy_toast = None;
        self.sync_current_document_tab();
    }

    pub(super) fn copy_selection(&mut self) {
        let Some(selection) = self.pointer.selection() else {
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
            self.effects.copy(CopyRequest::Selection { text });
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
        self.effects.copy(CopyRequest::Node {
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
            self.show_reference_chooser(references::ReferencePurpose::Copy);
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
        self.navigation.location(self.current_local_target())
    }

    fn current_local_target(&self) -> LocalTarget {
        let Some(target) = self
            .session
            .document
            .navigation()
            .get(self.selected)
            .map(|item| item.target_id.clone())
        else {
            return LocalTarget::Default;
        };
        if self.session.document.reference_target(&target).is_some() {
            LocalTarget::ReferenceOccurrence(target)
        } else {
            LocalTarget::Fragment(target)
        }
    }

    pub(super) fn request_open(&mut self, address: DocumentAddress, target: Option<String>) {
        if self.navigation.address() == Some(&address) {
            let current = self.current_location();
            let moved = if let Some(target) = target {
                self.jump_to_anchor(&target)
            } else {
                self.jump_content(false);
                true
            };
            if moved {
                self.navigation.commit(HistoryDirection::New, current);
            }
            return;
        }
        self.effects.open(NavigationRequest {
            document: address.into(),
            target: LocalTarget::from_fragment(target),
            direction: HistoryDirection::New,
        });
    }

    pub(super) fn navigate_history(&mut self, back: bool) {
        let Some((location, direction)) = self.navigation.plan_history(back) else {
            return;
        };
        if location.belongs_to(&self.session.current_bundle) {
            self.complete_local_history(&location, direction);
        } else if let Some(address) = location.address().cloned() {
            self.effects.open(NavigationRequest {
                document: address.into(),
                target: location.target().clone(),
                direction,
            });
        } else if let Some(bundle) = location.fallback() {
            self.complete_local_bundle(Arc::clone(bundle), location.target().clone(), direction);
        }
    }

    fn complete_local_history(&mut self, location: &HistoryLocation, direction: HistoryDirection) {
        if let Some(target) = location.target().id() {
            if matches!(location.target(), LocalTarget::ReferenceOccurrence(_)) {
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
        if let Some(target) = location.target().id() {
            self.reveal_anchor(target);
        }
    }

    fn complete_local_bundle(
        &mut self,
        bundle: Arc<ResolvedContent>,
        target: LocalTarget,
        direction: HistoryDirection,
    ) {
        self.complete_loaded_navigation(bundle, target, direction);
    }

    fn commit_history(&mut self, direction: HistoryDirection) {
        let current = self.current_location();
        self.navigation.commit(direction, current);
    }

    #[must_use]
    /// Return whether the terminal event loop should exit.
    pub const fn should_quit(&self) -> bool {
        self.quit
    }

    /// Advance due interaction timers using the host's clock. Performs no IO
    /// and does not sleep; the host remains responsible for drawing and input.
    pub fn tick(&mut self, now: Instant) -> UpdateOutcome {
        let mut outcome = UpdateOutcome::Unchanged;
        if let Some(column) = self.pointer.take_resize_due(now)
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

    /// Return the next timer delay relative to the host's clock, without
    /// waiting or reading terminal state.
    #[must_use]
    pub fn next_wakeup(&self, now: Instant) -> Option<Duration> {
        [
            self.navigation_sync_deadline,
            self.pointer.resize_deadline(),
            self.copy_toast.as_ref().map(|toast| toast.deadline),
            self.pointer
                .selection_scroll()
                .map(|scroll| scroll.deadline),
        ]
        .into_iter()
        .flatten()
        .map(|deadline| deadline.saturating_duration_since(now))
        .min()
    }
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
