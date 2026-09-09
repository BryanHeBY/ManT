//! Source-neutral application request construction for typed document navigation.
use mant_engine::LoadPolicy;
use mant_protocol::{DocumentAddress, QueryInput, QueryRequest, QueryView, RequestSchema};

pub(crate) fn request_for_address(address: &DocumentAddress) -> (QueryRequest, LoadPolicy) {
    let policy = match address {
        DocumentAddress::Markdown { .. } => LoadPolicy::Combined,
        DocumentAddress::Manual { .. } => LoadPolicy::ManualOnly,
    };
    (
        QueryRequest {
            schema: RequestSchema::V0Dot11,
            input: QueryInput::Document {
                // A resolved address must never degrade into source precedence or
                // suffix discovery when its exact destination is missing.
                selector: address.catalog_path(),
                source: None,
                manual_section: None,
            },
            view: QueryView::Full {},
        },
        policy,
    )
}

pub(crate) fn request_for_navigation(
    target: &mant_protocol::DocumentOpenTarget,
) -> (QueryRequest, LoadPolicy) {
    match target {
        mant_protocol::DocumentOpenTarget::Address { address } => request_for_address(address),
        mant_protocol::DocumentOpenTarget::Manual {
            name,
            manual_section,
        } => (
            QueryRequest {
                schema: RequestSchema::V0Dot11,
                input: QueryInput::Document {
                    selector: name.clone(),
                    source: None,
                    manual_section: manual_section.clone(),
                },
                view: QueryView::Full {},
            },
            LoadPolicy::ManualOnly,
        ),
    }
}
