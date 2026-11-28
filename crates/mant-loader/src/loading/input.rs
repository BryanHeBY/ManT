//! Resolves bounded direct files and in-memory document inputs.

#[cfg(feature = "roff")]
use super::parse_manual_bytes;
use super::{
    InputFormat, LoadError, LoadHost, LoadPolicy, LoadSpec, ManualLoadError, OsStr, Path,
    ResolvedContent, query_named_document,
};
use mant_codec::parse_markdown;

pub(super) fn load_with(
    spec: LoadSpec<'_>,
    policy: LoadPolicy,
    host: &dyn LoadHost,
) -> Result<ResolvedContent, LoadError> {
    load_impl(spec, policy, host, None)
}

pub(super) fn load_impl(
    spec: LoadSpec<'_>,
    policy: LoadPolicy,
    host: &dyn LoadHost,
    layout: Option<&mut Option<mant_codec::DecodedLinkDocument>>,
) -> Result<ResolvedContent, LoadError> {
    super::validate_load_spec(spec, policy)?;
    match spec {
        LoadSpec::Document {
            selector,
            source,
            manual_section,
        } => query_named_document(selector, source, manual_section, policy, host, layout),
        LoadSpec::File { path, format } => query_input_file(path, format, policy, host, layout),
    }
}

fn query_input_file(
    requested_path: &str,
    format: InputFormat,
    policy: LoadPolicy,
    host: &dyn LoadHost,
    layout: Option<&mut Option<mant_codec::DecodedLinkDocument>>,
) -> Result<ResolvedContent, LoadError> {
    let path = requested_path.trim();
    if path.is_empty() {
        return Err(LoadError::EmptyMarkdownPath);
    }
    let format = match format {
        InputFormat::Auto => {
            detect_input_format(path).ok_or_else(|| LoadError::UnsupportedInputFormat {
                path: path.to_owned(),
            })?
        }
        format => format,
    };
    match format {
        InputFormat::Markdown => query_markdown_file(path, policy, host),
        InputFormat::Roff => {
            if policy != LoadPolicy::Combined {
                return Err(LoadError::ConflictingSourceSelectors);
            }
            if !host.native_available() {
                return Err(LoadError::NativeBackendUnavailable { tldr_topic: None });
            }
            let input = Path::new(path);
            let decoded = if layout.is_some() {
                host.parse_manual_input_with_layout(input)
            } else {
                host.parse_manual_input(input)
                    .map(mant_codec::DecodedLinkDocument::from_plain)
            }
            .map_err(|detail| {
                LoadError::Manual(ManualLoadError::Parse {
                    name: path.to_owned(),
                    detail,
                })
            })?;
            let document = decoded.document();
            if document.sections.is_empty() && document.blocks.is_empty() {
                return Err(LoadError::NoReadableContent {
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
            let document = if let Some(layout) = layout {
                *layout = Some(decoded);
                None
            } else {
                Some(decoded.into_document())
            };
            Ok(ResolvedContent {
                label,
                address: None,
                document,
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
    policy: LoadPolicy,
    host: &dyn LoadHost,
) -> Result<ResolvedContent, LoadError> {
    let path = requested_path.trim();
    if path.is_empty() {
        return Err(LoadError::EmptyMarkdownPath);
    }
    if policy != LoadPolicy::Combined {
        return Err(LoadError::Markdown {
            path: path.to_owned(),
            detail: "content-only policies do not apply to direct input".to_owned(),
        });
    }
    let source = host
        .read_markdown(Path::new(path))
        .map_err(|detail| LoadError::Markdown {
            path: path.to_owned(),
            detail,
        })?;
    load_markdown_text(&source, Some(path.to_owned()))
}

/// Parse in-memory Markdown for the direct `mant -` command.
///
/// This helper accepts only caller-supplied text and labels; it has no request
/// schema or query view, and never resolves another document.
///
/// # Errors
///
/// Returns [`LoadError::EmptyMarkdown`] when parsing yields no document heading,
/// blocks, sections, or quick reference.
pub fn load_markdown_text(
    source: &str,
    source_path: Option<String>,
) -> Result<ResolvedContent, LoadError> {
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
    let parsed = parse_markdown(source, source_path).map_err(|error| LoadError::Markdown {
        path: error_path,
        detail: error.to_string(),
    })?;
    let document_is_empty = parsed.document.heading.is_none()
        && parsed.document.blocks.is_empty()
        && parsed.document.sections.is_empty();
    if document_is_empty && parsed.tldr.is_none() {
        return Err(LoadError::EmptyMarkdown {
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
#[cfg(feature = "roff")]
pub fn load_roff_bytes(source: &[u8]) -> Result<ResolvedContent, LoadError> {
    load_bounded_roff(source, |path, source| {
        parse_manual_bytes(path, source).map(mant_codec::DecodedLinkDocument::from_plain)
    })
    .map(|(label, decoded)| ResolvedContent {
        label,
        address: None,
        document: Some(decoded.into_document()),
        tldr: None,
    })
}

/// Parse one bounded roff stream and retain optional native link membership
/// paired with that exact legacy document snapshot.
/// # Errors
/// Returns the same limits or parse errors as ordinary bounded roff loading.
#[cfg(feature = "roff")]
pub fn load_roff_bytes_with_link_layout(
    source: &[u8],
) -> Result<super::ResolvedWithLinkLayout, LoadError> {
    load_bounded_roff(
        source,
        crate::manual_input::parse_manual_bytes_with_link_layout,
    )
    .map(|(label, decoded)| decoded.into_resolved(label, None, None))
}

#[cfg(feature = "roff")]
fn load_bounded_roff<E: ToString>(
    source: &[u8],
    parse: impl FnOnce(&Path, &[u8]) -> Result<mant_codec::DecodedLinkDocument, E>,
) -> Result<(String, mant_codec::DecodedLinkDocument), LoadError> {
    if u64::try_from(source.len()).unwrap_or(u64::MAX) > crate::MAX_MANUAL_BYTES {
        return Err(LoadError::Manual(ManualLoadError::Parse {
            name: "stdin".to_owned(),
            detail: format!(
                "roff input exceeds the {}-byte limit",
                crate::MAX_MANUAL_BYTES
            ),
        }));
    }
    let decoded = parse(Path::new("stdin"), source).map_err(|error| {
        LoadError::Manual(ManualLoadError::Parse {
            name: "stdin".to_owned(),
            detail: error.to_string(),
        })
    })?;
    let document = decoded.document();
    if document.sections.is_empty() && document.blocks.is_empty() {
        return Err(LoadError::NoReadableContent {
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
    Ok((label, decoded))
}
