#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod bounded;
mod catalog;
mod executable;
mod loading;
mod manual;
#[cfg(feature = "roff")]
mod manual_input;
mod manual_paths;
mod scope_load;
mod source;
mod tldr;

pub use catalog::{
    AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin, CatalogError,
    PreparedCatalogQuery, discover_documents, list_available_documents, query_available_documents,
};
pub use executable::{ExecutableLookup, find_host_executable};
#[cfg(feature = "roff")]
pub use loading::load_roff_bytes;
pub use loading::{
    DocumentLoader, LoadError, LoadPolicy, LoadSpec, MAX_MARKDOWN_BYTES, ManualLoadError,
    load_markdown_text, validate_load_spec,
};
pub use mant_ir::{ResolvedContent, is_manual_section};
pub use manual::{is_command_manual_section, parenthesized_manual_reference};
#[cfg(feature = "roff")]
pub use manual_input::{
    MAX_MANUAL_BYTES, ManualError, ManualErrorKind, parse_manual_bytes, parse_manual_page,
    parse_manual_source, parse_manual_source_with_report,
};
pub use manual_paths::{
    ManualPathDiagnostic, ManualRootDiscovery, discover_manual_roots, inspect_manual_roots,
};
pub use scope_load::{LoadedDocumentScope, ScopeLoadError, validate_document_scope};
pub use source::{LocateError, ManualIndex, ManualPage, ManualRequest, locate_manual_source_in};
pub use tldr::{
    HostPlatform, TldrCacheError, get_system_tldr_cache_dirs, get_tldr_cache_dir,
    get_tldr_languages, get_tldr_platforms, get_tldr_read_cache_dirs, normalize_tldr_topic,
    read_cached_tldr_page,
};
