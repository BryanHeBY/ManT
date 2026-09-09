#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod output;
mod query;
mod scope;
mod tldr;

#[cfg(feature = "roff")]
pub use mant_codec::lower_mandoc_document;
pub use mant_codec::{MarkdownParseError, ParsedMarkdown, TldrDirectiveError, parse_markdown};
pub use mant_ir::ResolvedContent;
pub use mant_loader::find_host_executable;
pub use mant_loader::{
    AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin, CatalogError,
    discover_documents, list_available_documents, query_available_documents,
};
pub use mant_loader::{
    DocumentLoader, LoadError, LoadPolicy, LoadSpec, MAX_MARKDOWN_BYTES, ManualLoadError,
};
pub use mant_loader::{LoadedDocumentScope, ScopeLoadError, validate_document_scope};
pub use mant_loader::{
    LocateError, ManualIndex, ManualPage, ManualRequest, locate_manual_source_in,
};
#[cfg(feature = "roff")]
pub use mant_loader::{
    MAX_MANUAL_BYTES, ManualError, ManualErrorKind, parse_manual_bytes, parse_manual_page,
    parse_manual_source, parse_manual_source_with_report,
};
pub use mant_loader::{
    ManualPathDiagnostic, ManualRootDiscovery, discover_manual_roots, inspect_manual_roots,
};
pub use mant_loader::{
    is_command_manual_section, is_manual_section, parenthesized_manual_reference,
};
pub use mant_query::{
    ExplanationError, explain_query, resolve_explanation_block, validate_explanation_query,
};
pub use mant_query::{
    ProjectionError, ReferenceProjectionLimits, SelectorCandidate, build_outline,
    build_outline_projection, build_outline_with_detail, build_outline_with_references,
    project_references, project_references_with_limits, select_excerpt, select_explanation,
    semantics_complete,
};
pub use mant_query::{
    QueryScopeView, ScopeExecutionError, ScopeInputError, explain_scope, search_scope,
};
pub use mant_query::{SearchError, search_query, validate_search_query};
pub use output::{
    MarkdownFragmentOptions, MarkdownOptions, SearchTextRole, render_excerpt_json,
    render_excerpt_markdown, render_excerpt_markdown_with_options, render_excerpt_text,
    render_excerpt_text_with, render_explanation_markdown, render_explanation_text,
    render_explanation_text_with, render_markdown, render_markdown_with_options,
    render_outline_entry_summary, render_outline_json, render_outline_markdown,
    render_outline_relationships, render_outline_text, render_outline_text_with, render_query_json,
    render_query_man, render_query_text, render_query_text_with, render_scope_explanation_markdown,
    render_scope_explanation_text, render_scope_explanation_text_with, render_search_json,
    render_search_markdown, render_search_text, render_search_text_with, render_update_json,
};
#[cfg(feature = "roff")]
pub use query::query_roff_bytes;
pub use query::{
    DocumentResolver, QueryError, QueryExecutionError, QueryValidationError, QueryViewResult,
    execute_query, project_query_view, query_markdown_text, resolve_query,
    resolve_query_with_policy, validate_query_request,
};
pub use scope::{ScopeQueryError, execute_scope_query, validate_scope_query_request};
pub use tldr::{
    HostPlatform, TldrCacheError, TldrPageLocation, TldrParseError, get_system_tldr_cache_dirs,
    get_tldr_cache_dir, get_tldr_languages, get_tldr_platforms, get_tldr_read_cache_dirs,
    normalize_tldr_topic, parse_tldr_command, parse_tldr_page, read_cached_tldr_page,
};
#[cfg(feature = "tldr-update")]
pub use tldr::{TldrUpdateError, update_tldr_cache};

/// Reports the native contract version through the engine layer.
#[must_use]
pub const fn native_api_version() -> &'static str {
    mant_protocol::NATIVE_API_VERSION
}

#[cfg(test)]
mod tests {
    use super::native_api_version;

    #[test]
    fn exposes_the_native_api_version() {
        assert_eq!(native_api_version(), "0.11");
    }
}
