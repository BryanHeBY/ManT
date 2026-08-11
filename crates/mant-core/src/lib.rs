//! Manual source, query, and output engine independent from its process hosts.

mod bounded;
mod catalog;
mod definitions;
mod executable;
mod inline;
mod mandoc;
mod markdown;
mod output;
mod projection;
mod query;
mod search;
mod source;
mod text_safety;
mod tldr;

pub use catalog::{
    AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin, list_available_documents,
};
pub use mandoc::{
    MAX_MANUAL_BYTES, ManualError, ManualErrorKind, lower_mandoc_document, parse_manual_page,
    parse_manual_source,
};
pub use markdown::{MarkdownParseError, ParsedMarkdown, TldrDirectiveError, parse_markdown};
pub use output::{
    MarkdownOptions, render_excerpt_json, render_excerpt_markdown,
    render_excerpt_markdown_with_options, render_excerpt_text, render_markdown,
    render_markdown_with_options, render_outline_json, render_outline_markdown,
    render_outline_text, render_query_json, render_query_man, render_query_text,
    render_search_json, render_search_markdown, render_search_text, render_update_json,
};
pub use projection::{
    ProjectionError, SelectorCandidate, build_outline, build_outline_with_detail, select_excerpt,
    select_explanation,
};
pub use query::{
    DocumentResolver, MAX_MARKDOWN_BYTES, ManualLoadError, QueryError, QueryExecutionError,
    QueryPolicy, QueryViewResult, execute_query, project_query_view, query_markdown_text,
    resolve_query, resolve_query_with_policy, validate_query_request,
};
pub use search::{SearchError, search_query, validate_search_query};
pub use source::{
    LocateError, ManualIndex, ManualPage, ManualRequest, discover_manual_roots,
    locate_manual_source_in,
};
pub use tldr::{
    HostPlatform, TldrCacheError, TldrPageLocation, TldrParseError, TldrUpdateError,
    get_system_tldr_cache_dirs, get_tldr_cache_dir, get_tldr_languages, get_tldr_platforms,
    get_tldr_read_cache_dirs, normalize_tldr_topic, parse_tldr_command, parse_tldr_page,
    read_cached_tldr_page, update_tldr_cache,
};

/// Reports the native contract version through the engine layer.
#[must_use]
pub const fn native_api_version() -> &'static str {
    mant_ast::NATIVE_API_VERSION
}

#[cfg(test)]
mod tests {
    use super::native_api_version;

    #[test]
    fn exposes_the_ast_contract_version() {
        assert_eq!(native_api_version(), "6");
    }
}
