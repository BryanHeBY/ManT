//! Manual source, query, and output engine independent from its process hosts.

mod definitions;
mod groff_html;
mod mandoc;
mod output;
mod projection;
mod query;
mod search;
mod source;
mod tldr;

pub use groff_html::parse_groff_html;
pub use mandoc::{lower_mandoc_document, parse_manual_source};
pub use output::{
    MarkdownOptions, render_excerpt_json, render_excerpt_markdown,
    render_excerpt_markdown_with_options, render_excerpt_text, render_markdown,
    render_markdown_with_options, render_outline_json, render_outline_markdown,
    render_outline_text, render_query_json, render_query_text, render_search_json,
    render_search_markdown, render_search_text, render_update_json,
};
pub use projection::{ProjectionError, build_outline, build_outline_with_detail, select_excerpt};
pub use query::{QueryError, QueryPolicy, query, query_with_policy};
pub use search::{SearchError, search_query, validate_search_query};
pub use source::{
    CommandOutput, CommandRunner, LocateError, ManualRequest, SystemCommandRunner,
    locate_manual_source, locate_manual_source_with,
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
        assert_eq!(native_api_version(), "2");
    }
}
