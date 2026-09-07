//! Current document layout state and explicit document-change policy.
use super::{Arc, DocumentView, HashMap, HistoryDirection, RenderedDocument, ResolvedContent};

pub(super) struct DocumentSession {
    pub(super) current_bundle: Arc<ResolvedContent>,
    pub(super) document: DocumentView,
    pub(super) content_scroll: usize,
    pub(super) content_render_width: u16,
    pub(super) rendered_cache: HashMap<u16, RenderedDocument>,
}
impl DocumentSession {
    pub(super) fn new(current_bundle: Arc<ResolvedContent>, document: DocumentView) -> Self {
        Self {
            current_bundle,
            document,
            content_scroll: 0,
            content_render_width: 0,
            rendered_cache: HashMap::new(),
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DocumentChangeReason {
    Open,
    History,
    SearchResult,
}
impl From<HistoryDirection> for DocumentChangeReason {
    fn from(direction: HistoryDirection) -> Self {
        match direction {
            HistoryDirection::New => Self::Open,
            HistoryDirection::Back | HistoryDirection::Forward => Self::History,
        }
    }
}
