//! Resolves bounded direct files and in-memory document inputs.

use super::{
    InputFormat, ManualLoadError, OsStr, Path, QueryError, QueryHost, QueryInput, QueryPolicy,
    QueryRequest, ResolvedContent, parse_manual_bytes, parse_markdown, query_named_document,
};

pub(super) fn query_with(
    request: &QueryRequest,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    match &request.input {
        QueryInput::Document {
            selector,
            source,
            manual_section,
        } => query_named_document(
            selector,
            source.as_deref(),
            manual_section.as_deref(),
            policy,
            host,
        ),
        QueryInput::File { path, format } => query_input_file(path, *format, policy, host),
    }
}

fn query_input_file(
    requested_path: &str,
    format: InputFormat,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let path = requested_path.trim();
    if path.is_empty() {
        return Err(QueryError::EmptyMarkdownPath);
    }
    let format = match format {
        InputFormat::Auto => {
            detect_input_format(path).ok_or_else(|| QueryError::UnsupportedInputFormat {
                path: path.to_owned(),
            })?
        }
        format => format,
    };
    match format {
        InputFormat::Markdown => query_markdown_file(path, policy, host),
        InputFormat::Roff => {
            if policy != QueryPolicy::Combined {
                return Err(QueryError::ConflictingSourceSelectors);
            }
            let document = host.parse_manual_input(Path::new(path)).map_err(|detail| {
                QueryError::Manual(ManualLoadError::Parse {
                    name: path.to_owned(),
                    detail,
                })
            })?;
            if document.sections.is_empty() && document.blocks.is_empty() {
                return Err(QueryError::NoReadableContent {
                    name: path.to_owned(),
                });
            }
            let label = document
                .meta
                .names
                .first()
                .cloned()
                .or_else(|| document.meta.title.clone())
                .unwrap_or_else(|| input_file_label(path));
            Ok(ResolvedContent {
                label,
                address: None,
                document: Some(document),
                tldr: None,
            })
        }
        InputFormat::Auto => unreachable!("auto input was resolved above"),
    }
}

fn detect_input_format(path: &str) -> Option<InputFormat> {
    let mut name = Path::new(path).file_name()?.to_str()?.to_ascii_lowercase();
    let mut compressed = false;
    if Path::new(&name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| matches!(extension, "gz" | "zst"))
    {
        name = Path::new(&name).file_stem()?.to_str()?.to_owned();
        compressed = true;
    }
    let extension = Path::new(&name).extension()?.to_str()?;
    if matches!(extension, "md" | "markdown") {
        return (!compressed).then_some(InputFormat::Markdown);
    }
    if matches!(extension, "roff" | "man" | "mdoc") {
        return Some(InputFormat::Roff);
    }
    crate::is_manual_section(extension).then_some(InputFormat::Roff)
}

fn input_file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(path)
        .to_owned()
}

fn query_markdown_file(
    requested_path: &str,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let path = requested_path.trim();
    if path.is_empty() {
        return Err(QueryError::EmptyMarkdownPath);
    }
    if policy != QueryPolicy::Combined {
        return Err(QueryError::Markdown {
            path: path.to_owned(),
            detail: "content-only policies do not apply to direct input".to_owned(),
        });
    }
    let source = host
        .read_markdown(Path::new(path))
        .map_err(|detail| QueryError::Markdown {
            path: path.to_owned(),
            detail,
        })?;
    query_markdown_text(&source, Some(path.to_owned()))
}

/// Parse in-memory Markdown for the direct `mant -` command.
///
/// This helper intentionally sits outside [`QueryRequest`]: public protocol
/// requests reference local files and never embed arbitrary document content.
///
/// # Errors
///
/// Returns [`QueryError::EmptyMarkdown`] when parsing yields no visible blocks
/// or sections.
pub fn query_markdown_text(
    source: &str,
    source_path: Option<String>,
) -> Result<ResolvedContent, QueryError> {
    let label = source_path.as_deref().map_or_else(
        || "stdin".to_owned(),
        |path| {
            Path::new(path)
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or(path)
                .to_owned()
        },
    );
    let error_path = source_path.clone().unwrap_or_else(|| "stdin".to_owned());
    let parsed = parse_markdown(source, source_path).map_err(|error| QueryError::Markdown {
        path: error_path,
        detail: error.to_string(),
    })?;
    let document_is_empty =
        parsed.document.blocks.is_empty() && parsed.document.sections.is_empty();
    if document_is_empty && parsed.tldr.is_none() {
        return Err(QueryError::EmptyMarkdown {
            label: label.clone(),
        });
    }
    Ok(ResolvedContent {
        address: None,
        label,
        document: (!document_is_empty).then_some(parsed.document),
        tldr: parsed.tldr,
    })
}

/// Parse one bounded roff stream without consulting MANPATH or following `.so`.
///
/// # Errors
///
/// Returns a native parse error or an empty-document error.
pub fn query_roff_bytes(source: &[u8]) -> Result<ResolvedContent, QueryError> {
    if u64::try_from(source.len()).unwrap_or(u64::MAX) > crate::MAX_MANUAL_BYTES {
        return Err(QueryError::Manual(ManualLoadError::Parse {
            name: "stdin".to_owned(),
            detail: format!(
                "roff input exceeds the {}-byte limit",
                crate::MAX_MANUAL_BYTES
            ),
        }));
    }
    let document = parse_manual_bytes(Path::new("stdin"), source).map_err(|error| {
        QueryError::Manual(ManualLoadError::Parse {
            name: "stdin".to_owned(),
            detail: error.to_string(),
        })
    })?;
    if document.sections.is_empty() && document.blocks.is_empty() {
        return Err(QueryError::NoReadableContent {
            name: "stdin".to_owned(),
        });
    }
    let label = document
        .meta
        .names
        .first()
        .cloned()
        .or_else(|| document.meta.title.clone())
        .unwrap_or_else(|| "stdin".to_owned());
    Ok(ResolvedContent {
        address: None,
        label,
        document: Some(document),
        tldr: None,
    })
}
