//! View-independent local acquisition specifications and host seam.
use super::LoadError;
use crate::{ManualPage, ManualRequest};
use mant_ir::{Document, DocumentAddress, TldrDocument};
use mant_protocol::{
    InputFormat, MAX_DOCUMENT_SELECTOR_CHARS, MAX_SOURCE_SELECTOR_CHARS, ScopeTextError,
    validate_scope_text,
};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};

/// Borrowed loading input without a protocol schema or a query view.
#[derive(Debug, Clone, Copy)]
pub enum LoadSpec<'a> {
    /// Resolve an explicitly qualified logical name.
    Document {
        /// Requested logical selector.
        selector: &'a str,
        /// Optional registered source namespace.
        source: Option<&'a str>,
        /// Optional native manual category.
        manual_section: Option<&'a str>,
    },
    /// Read an explicitly authorized local file.
    File {
        /// Caller-supplied source path.
        path: &'a str,
        /// Explicit or inferred source format.
        format: InputFormat,
    },
}

/// Upper bound on a single Markdown source, shared by every input path.
///
/// File and stdin readers both enforce this so an unbounded source (a pipe, a
/// character device such as `/dev/zero`, or a pathologically large file) cannot
/// exhaust memory. A file's reported length is not trusted: some sources report
/// zero yet stream without end, so readers cap the byte count directly.
pub const MAX_MARKDOWN_BYTES: u64 = 16 * 1024 * 1024;

/// Closed content-resolution policy kept outside the serialized request contract.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QueryPolicy {
    /// Resolve a full document and attach a compatible quick reference.
    #[default]
    Combined,
    /// Bypass registered Markdown and tldr content.
    ManualOnly,
    /// Resolve only embedded or cached tldr content through source precedence.
    TldrOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FullDocumentMode {
    Priority,
    NativeManual,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QuickReferenceMode {
    AttachToCommandManual,
    Exclude,
    Only,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NamedResolutionPlan {
    pub(super) document: FullDocumentMode,
    pub(super) quick_reference: QuickReferenceMode,
}

impl QueryPolicy {
    pub(super) fn named_resolution_plan(self, has_manual_section: bool) -> NamedResolutionPlan {
        match self {
            Self::Combined => NamedResolutionPlan {
                document: if has_manual_section {
                    FullDocumentMode::NativeManual
                } else {
                    FullDocumentMode::Priority
                },
                quick_reference: QuickReferenceMode::AttachToCommandManual,
            },
            Self::ManualOnly => NamedResolutionPlan {
                document: FullDocumentMode::NativeManual,
                quick_reference: QuickReferenceMode::Exclude,
            },
            Self::TldrOnly => NamedResolutionPlan {
                document: FullDocumentMode::None,
                quick_reference: QuickReferenceMode::Only,
            },
        }
    }
}

pub(super) trait LoadHost {
    fn name_candidates(&self, name: &str) -> Vec<String>;
    fn locate_registered_document(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Option<RegisteredSelection>, String>;
    fn locate_registered_document_groups(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Vec<RegisteredSelectionGroup>, String>;
    fn locate_registered_address(
        &self,
        address: &DocumentAddress,
    ) -> Result<Option<RegisteredSelection>, String>;
    fn locate_manual(&self, request: &ManualRequest) -> Result<ManualPage, String>;
    fn parse_manual(&self, page: &ManualPage) -> Result<Document, String>;
    fn parse_manual_input(&self, path: &Path) -> Result<Document, String>;
    fn read_tldr(&self, name: &str) -> Result<Option<TldrDocument>, String>;
    fn read_markdown(&self, path: &Path) -> Result<String, String>;
}

#[derive(Clone, Copy)]
pub(super) enum RegisteredLookupPhase {
    BeforeBuiltin,
    AfterBuiltin,
}

#[derive(Clone)]
pub(super) struct RegisteredSelection {
    pub(super) path: PathBuf,
    pub(super) address: DocumentAddress,
}

pub(super) struct RegisteredSelectionGroup {
    pub(super) documents: Vec<RegisteredSelection>,
}

pub(super) struct LoadedManual {
    pub(super) document: Document,
    pub(super) address: DocumentAddress,
}

/// Read at most `limit` bytes of UTF-8, rejecting anything larger.
///
/// The reader is bounded directly instead of trusting a reported length: a pipe
/// or character device such as `/dev/zero` reports no size yet streams without
/// end, so only capping the byte count keeps the read finite.
pub(super) fn read_capped_utf8(reader: impl Read, limit: u64) -> Result<String, String> {
    read_capped_utf8_io(reader, limit).map_err(|error| error.to_string())
}

/// Read bounded UTF-8 while preserving failures from the underlying reader.
pub(crate) fn read_capped_utf8_io(reader: impl Read, limit: u64) -> io::Result<String> {
    crate::bounded::read_utf8(reader, limit, "Markdown document")
}

/// Validate source-selection constraints before any host access.
///
/// # Errors
/// Returns the precise loading input violation; no query view is inspected.
pub fn validate_load_spec(spec: LoadSpec<'_>, policy: QueryPolicy) -> Result<(), LoadError> {
    match spec {
        LoadSpec::Document {
            selector,
            source,
            manual_section,
        } => {
            validate_scope_text(selector, MAX_DOCUMENT_SELECTOR_CHARS).map_err(|error| {
                if error == ScopeTextError::Empty {
                    LoadError::EmptyName
                } else {
                    LoadError::InvalidSelector {
                        field: "document selector",
                        error,
                    }
                }
            })?;
            if let Some(source) = source {
                validate_scope_text(source, MAX_SOURCE_SELECTOR_CHARS).map_err(|error| {
                    if error == ScopeTextError::Empty {
                        LoadError::InvalidSource
                    } else {
                        LoadError::InvalidSelector {
                            field: "document source",
                            error,
                        }
                    }
                })?;
            }
            if manual_section.is_some_and(|value| !crate::is_manual_section(value.trim())) {
                return Err(LoadError::InvalidManualSection);
            }
            if policy == QueryPolicy::TldrOnly
                && let Some(section) = manual_section
                && !crate::is_command_manual_section(section.trim())
            {
                return Err(LoadError::TldrManualSection {
                    section: section.trim().to_owned(),
                });
            }
            if source.is_some() && (manual_section.is_some() || policy == QueryPolicy::ManualOnly) {
                return Err(LoadError::ConflictingSourceSelectors);
            }
        }
        LoadSpec::File { path, .. } => {
            if path.trim().is_empty() {
                return Err(LoadError::EmptyMarkdownPath);
            }
            if policy != QueryPolicy::Combined {
                return Err(LoadError::Markdown {
                    path: path.trim().to_owned(),
                    detail: "content-only policies do not apply to direct input".to_owned(),
                });
            }
        }
    }
    Ok(())
}
