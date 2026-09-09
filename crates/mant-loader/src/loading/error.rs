//! Loading failures own acquisition context, never query-view errors.
use mant_protocol::ScopeTextError;
use std::{error::Error, fmt, path::PathBuf};

/// Invalid loading input or failure to acquire readable local content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Native input was selected but this build does not enable the roff backend.
    NativeBackendUnavailable {
        /// Optional cached quick reference available through explicit tldr policy.
        tldr_topic: Option<String>,
    },
    /// A document selector was empty after trimming.
    EmptyName,
    /// A native manual category was empty or malformed.
    InvalidManualSection,
    /// A tldr command query was qualified by a non-command manual section.
    TldrManualSection {
        /// Incompatible native manual section.
        section: String,
    },
    /// An explicit Markdown source name was empty.
    InvalidSource,
    /// Markdown-source and native-manual selectors were combined.
    ConflictingSourceSelectors,
    /// A direct Markdown input path was empty.
    EmptyMarkdownPath,
    /// Automatic format inference did not recognize a direct input.
    UnsupportedInputFormat {
        /// Caller-facing input path.
        path: String,
    },
    /// A document selector or source violated its bounded loading contract.
    InvalidSelector {
        /// Loading input field name.
        field: &'static str,
        /// Precise bound or character violation.
        error: ScopeTextError,
    },
    /// Markdown input could not be read or parsed.
    Markdown {
        /// Caller-facing source path.
        path: String,
        /// Stable failure detail.
        detail: String,
    },
    /// Markdown parsing produced neither document nor tldr content.
    EmptyMarkdown {
        /// Selected-document label.
        label: String,
    },
    /// Registered-document discovery failed.
    Registry {
        /// Stable source-configuration or discovery detail.
        detail: String,
    },
    /// Native manual loading failed.
    Manual(ManualLoadError),
    /// No full document was found, but an optional tldr entry is available.
    ManualWithTldr {
        /// Native-manual failure retained as the authoritative lookup error.
        error: ManualLoadError,
        /// Topic that can be queried explicitly with `--tldr`.
        topic: String,
    },
    /// An explicit tldr query found no quick-reference candidate.
    TldrNotFound {
        /// Requested tldr topic.
        topic: String,
    },
    /// An explicit tldr candidate could not be read or parsed.
    Tldr {
        /// Requested tldr topic.
        topic: String,
        /// Stable cache or Markdown failure detail.
        detail: String,
    },
    /// No Markdown, manual, or quick-reference content could be resolved.
    NoReadableContent {
        /// Requested document name.
        name: String,
    },
}

/// Native-manual resolution or lowering failed after candidate selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualLoadError {
    /// No indexed native manual matched the request.
    NotFound {
        /// Requested manual name.
        name: String,
        /// Search-path and candidate detail.
        detail: String,
    },
    /// A selected manual could not be parsed or lowered.
    Parse {
        /// Requested manual name.
        name: String,
        /// Stable parser or source-policy detail.
        detail: String,
    },
    /// Parsing succeeded but produced no readable semantic content.
    Empty {
        /// Requested manual name.
        name: String,
        /// Physical selected manual path.
        path: PathBuf,
        /// Non-fatal parser findings explaining the empty result.
        diagnostics: Vec<String>,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NativeBackendUnavailable { tldr_topic } => {
                formatter.write_str("native manual loading requires the 'roff' feature")?;
                if let Some(topic) = tldr_topic {
                    write!(
                        formatter,
                        "\nhint: a tldr entry is available; run `mant {topic} --tldr`"
                    )?;
                }
                Ok(())
            }
            Self::EmptyName => formatter.write_str("name must not be empty"),
            Self::InvalidManualSection => formatter.write_str(
                "manual section must be a conventional number or the single letter 'l' or 'n'",
            ),
            Self::TldrManualSection { section } => write!(
                formatter,
                "manual section '{section}' does not identify a command quick reference; tldr supports section families 1 and 8"
            ),
            Self::InvalidSource => formatter.write_str("document source must not be empty"),
            Self::ConflictingSourceSelectors => formatter.write_str(
                "document source cannot be combined with a manual section or manual-only policy",
            ),
            Self::EmptyMarkdownPath => formatter.write_str("Markdown path must not be empty"),
            Self::UnsupportedInputFormat { path } => write!(
                formatter,
                "could not infer the input format for '{path}'; use --input-format markdown or roff"
            ),
            Self::InvalidSelector { field, error } => {
                write!(formatter, "{field} {}", selector_error_message(*error))
            }
            Self::Markdown { path, detail } => {
                write!(
                    formatter,
                    "could not load Markdown document '{path}': {detail}"
                )
            }
            Self::EmptyMarkdown { label } => {
                write!(
                    formatter,
                    "Markdown document '{label}' has no readable content"
                )
            }
            Self::Registry { detail } => formatter.write_str(detail),
            Self::Manual(error) => error.fmt(formatter),
            Self::ManualWithTldr { error, topic } => {
                error.fmt(formatter)?;
                write!(
                    formatter,
                    "\nhint: a tldr entry is available; run `mant {topic} --tldr`"
                )
            }
            Self::TldrNotFound { topic } => {
                write!(formatter, "no tldr quick reference was found for '{topic}'")
            }
            Self::Tldr { topic, detail } => {
                write!(formatter, "could not load tldr entry '{topic}': {detail}")
            }
            Self::NoReadableContent { name } => {
                write!(
                    formatter,
                    "no readable document content was found for '{name}'"
                )
            }
        }
    }
}

impl Error for LoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Manual(error) | Self::ManualWithTldr { error, .. } => Some(error),
            _ => None,
        }
    }
}

fn selector_error_message(error: ScopeTextError) -> String {
    match error {
        ScopeTextError::Empty => "must not be empty".to_owned(),
        ScopeTextError::ControlCharacter => "must not contain control characters".to_owned(),
        ScopeTextError::TooLong { maximum } => {
            format!("must not exceed {maximum} Unicode scalar values")
        }
    }
}

impl fmt::Display for ManualLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { name, detail } => {
                write!(formatter, "could not load manual '{name}': {detail}")
            }
            Self::Parse { name, detail } => write!(
                formatter,
                "could not load manual '{name}': manual source: {detail}"
            ),
            Self::Empty {
                name,
                path,
                diagnostics,
            } => {
                write!(
                    formatter,
                    "could not load manual '{name}': libmandoc parsed {} but produced no readable sections",
                    path.display()
                )?;
                if !diagnostics.is_empty() {
                    write!(formatter, "; diagnostics: {}", diagnostics.join("; "))?;
                }
                Ok(())
            }
        }
    }
}

impl Error for ManualLoadError {}
