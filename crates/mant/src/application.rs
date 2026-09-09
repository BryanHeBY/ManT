//! Complete-request application calls and typed document navigation.
use crate::{
    error::{Failure, query_failure, scope_query_failure},
    host::CliHost,
};
use mant_engine::{LoadPolicy, PreparedQueryRequest, PreparedScopeQuery, QueryViewResult};
#[cfg(feature = "tui")]
use mant_ir::ResolvedContent;
#[cfg(any(feature = "tui", test))]
use mant_protocol::{DocumentAddress, QueryInput, QueryView, RequestSchema};
use mant_protocol::{QueryRequest, ScopeQueryRequest, ScopeQueryResponse};

pub(crate) fn execute_query(
    request: &QueryRequest,
    policy: LoadPolicy,
    host: &dyn CliHost,
) -> Result<QueryViewResult, Failure> {
    let prepared = PreparedQueryRequest::new(request, policy).map_err(query_failure)?;
    host.query(&prepared)
}

pub(crate) fn execute_scope_query(
    request: &ScopeQueryRequest,
    host: &dyn CliHost,
) -> Result<ScopeQueryResponse, Failure> {
    let prepared = PreparedScopeQuery::new(request).map_err(scope_query_failure)?;
    host.query_scope(&prepared)
}

#[cfg(feature = "tui")]
pub(crate) fn read_full(
    request: &QueryRequest,
    policy: LoadPolicy,
    host: &dyn CliHost,
) -> Result<ResolvedContent, Failure> {
    if !matches!(request.view, QueryView::Full {}) {
        return Err(Failure::usage(
            "interactive mode requires the complete document view",
        ));
    }
    match execute_query(request, policy, host)? {
        QueryViewResult::Full(content) => Ok(*content),
        _ => Err(Failure::operational(
            "complete document request returned a projected view",
        )),
    }
}

#[cfg(any(feature = "tui", test))]
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

#[cfg(any(feature = "tui", test))]
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
