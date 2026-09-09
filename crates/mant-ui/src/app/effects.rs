//! Four orthogonal pending capabilities, each retaining the latest intent.
//! `ReaderServices` drains discovery, opening, external opening, then copying.

use super::NavigationRequest;
use crate::{CopyRequest, ExternalUri};
use mant_protocol::CatalogQuery;

#[derive(Default)]
pub(super) struct PendingEffects {
    discovery: Option<CatalogQuery>,
    open: Option<NavigationRequest>,
    external: Option<ExternalUri>,
    copy: Option<CopyRequest>,
}

impl PendingEffects {
    pub(super) fn discover(&mut self, request: CatalogQuery) {
        self.discovery = Some(request);
    }
    pub(super) fn open(&mut self, request: NavigationRequest) {
        self.open = Some(request);
    }
    pub(super) fn external(&mut self, request: ExternalUri) {
        self.external = Some(request);
    }
    pub(super) fn copy(&mut self, request: CopyRequest) {
        self.copy = Some(request);
    }
    pub(super) fn cancel_discovery(&mut self) {
        self.discovery = None;
    }
    pub(super) fn take_discovery(&mut self) -> Option<CatalogQuery> {
        self.discovery.take()
    }
    pub(super) fn take_open(&mut self) -> Option<NavigationRequest> {
        self.open.take()
    }
    pub(super) fn take_external(&mut self) -> Option<ExternalUri> {
        self.external.take()
    }
    pub(super) fn take_copy(&mut self) -> Option<CopyRequest> {
        self.copy.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{HistoryDirection, LocalTarget};
    use mant_protocol::{DocumentAddress, DocumentOpenTarget};

    #[test]
    fn replacing_or_cancelling_one_slot_preserves_other_pending_capabilities() {
        let mut effects = PendingEffects::default();
        effects.discover(CatalogQuery::default());
        effects.open(NavigationRequest {
            document: DocumentOpenTarget::Address {
                address: DocumentAddress::Manual {
                    name: "kept".into(),
                    manual_section: "1".into(),
                },
            },
            target: LocalTarget::Default,
            direction: HistoryDirection::New,
        });
        effects.external(ExternalUri::parse("https://example.test/").unwrap());
        effects.copy(CopyRequest::Selection {
            text: "older".into(),
        });
        effects.copy(CopyRequest::Selection {
            text: "latest".into(),
        });
        effects.cancel_discovery();
        assert!(effects.take_discovery().is_none());
        assert_eq!(
            effects.take_open().unwrap().address(),
            &DocumentAddress::Manual {
                name: "kept".into(),
                manual_section: "1".into(),
            }
        );
        assert_eq!(
            effects.take_external().unwrap().as_str(),
            "https://example.test/"
        );
        let CopyRequest::Selection { text } = effects.take_copy().unwrap() else {
            panic!("selection");
        };
        assert_eq!(text, "latest");
        assert!(effects.take_open().is_none());
        assert!(effects.take_external().is_none());
        assert!(effects.take_copy().is_none());
    }
}
