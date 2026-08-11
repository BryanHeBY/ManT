//! Local Markdown registry and transactional source acquisition for `ManT`.

#[cfg(feature = "update")]
mod archive;
mod bounded;
mod config;
#[cfg(feature = "update")]
mod download;
mod registry;
#[cfg(feature = "update")]
mod update;

pub use config::{
    ConfiguredSource, DocumentPaths, SourceConfig, SourceConfigError, SourceLocation,
    document_paths, load_source_config,
};
pub use registry::{
    RegisteredDocument, RegisteredDocumentOrigin, find_registered_document,
    list_registered_documents,
};
#[cfg(feature = "update")]
pub use update::{
    DocumentSourcesUpdate, DocumentSourcesUpdateSchema, SourceUpdateAction, SourceUpdateResult,
    update_document_sources,
};

pub(crate) use config::{SOURCE_METADATA_FILE, is_source_name};
