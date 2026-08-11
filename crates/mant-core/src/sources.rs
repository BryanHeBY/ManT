//! Configures, discovers, and updates local Markdown document repositories.

mod config;
mod registry;
mod update;

pub use config::{
    DocumentPaths, RepositorySource, SourceConfig, SourceConfigError, document_paths,
    load_source_config,
};
pub use registry::{
    RegisteredDocument, RegisteredDocumentOrigin, find_registered_document,
    list_registered_documents,
};
pub use update::{
    DocumentSourcesUpdate, DocumentSourcesUpdateSchema, SourceUpdateAction, SourceUpdateResult,
    update_document_sources,
};

pub(crate) use config::{SOURCE_METADATA_FILE, is_source_name};
