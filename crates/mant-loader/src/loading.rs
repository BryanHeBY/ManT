//! View-independent local source acquisition and its private precedence drivers.
use crate::{
    ManualIndex, ManualPage, ManualRequest, discover_manual_roots,
    executable::query_name_candidates, locate_manual_source_in, read_cached_tldr_page,
};
#[cfg(feature = "roff")]
use crate::{parse_manual_bytes, parse_manual_page, parse_manual_source};
use mant_ir::{Document, DocumentAddress, MarkdownOrigin, ResolvedContent, TldrDocument};
use mant_protocol::{CatalogQuery, DocumentCatalog, InputFormat};
use mant_sources::{RegisteredDocumentIndex, RegisteredDocumentOrigin, SourceConfigError};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};
mod error;
mod input;
mod named;
mod resolver;
mod spec;
#[cfg(test)]
mod tests;
pub use error::{LoadError, ManualLoadError};
pub use input::load_markdown_text;
#[cfg(feature = "roff")]
pub use input::load_roff_bytes;
use input::load_with;
use named::query_named_document;
pub use resolver::DocumentLoader;
#[cfg(test)]
use spec::read_capped_utf8_io;
use spec::{
    FullDocumentMode, LoadHost, LoadedManual, QuickReferenceMode, RegisteredLookupPhase,
    RegisteredSelection, RegisteredSelectionGroup, read_capped_utf8,
};
pub use spec::{LoadPolicy, LoadSpec, MAX_MARKDOWN_BYTES, validate_load_spec};
