#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

#[cfg(feature = "update")]
mod archive;
mod bounded;
mod config;
#[cfg(feature = "update")]
mod download;
mod inspection;
#[cfg(feature = "update")]
mod limits;
mod metadata;
mod registry;
#[cfg(feature = "update")]
mod update;

pub use config::{
    ConfiguredSource, DocumentPaths, SourceConfig, SourceConfigError, SourceLocation,
    document_paths, load_source_config,
};
pub use inspection::{
    ConfiguredSourceInspection, DocumentSourcesInspection, OrphanedSourceInspection,
    SourceInstallationStatus, SourceTransport, inspect_document_sources,
};
pub use registry::{
    BUILTIN_CONTENT_PRIORITY, RegisteredDocument, RegisteredDocumentIndex, RegisteredDocumentMatch,
    RegisteredDocumentOrigin, find_registered_document_candidates, list_registered_documents,
};
#[cfg(feature = "update")]
pub use update::{
    DocumentSourcesPrune, DocumentSourcesPruneSchema, DocumentSourcesUpdate,
    DocumentSourcesUpdateSchema, OrphanedSource, SourcePruneAction, SourcePruneResult,
    SourceUpdateAction, SourceUpdateResult, prune_document_sources, update_document_sources,
};

pub(crate) use config::{SOURCE_METADATA_FILE, is_source_name};
