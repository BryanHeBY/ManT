//! Parses a conservative Markdown subset into the shared document contract.
//!
//! Supported syntax becomes semantic IR nodes. Recognized extensions outside
//! the subset remain visible as exact source text with an attached diagnostic.

mod bindings;
mod blocks;
mod container;
mod directives;
mod entries;
mod events;
mod headings;
mod inline;
mod layout;
mod metadata;
mod source;
mod structure;

#[cfg(test)]
mod tests;

pub use container::TldrDirectiveError;
pub(crate) use entries::export_attached_policy;
pub(crate) use metadata::export_entry_metadata;

use mant_ir::DOCUMENT_ROOT_ID;
use std::{error::Error, fmt};

use mant_ir::{
    Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource, ParserInfo, Section,
    SourceFormat, TldrDocument, TldrOrigin, validate_document,
};
#[cfg(test)]
use pulldown_cmark::Parser;

use self::{
    container::split_markdown,
    directives::PreparedMarkdown,
    entries::normalize_entry_lists,
    events::{EventCursor, SpannedEvent, markdown_options},
    headings::{extract_document_title, nest_sections},
    layout::normalize_markdown_layout,
    source::MarkdownSource,
    structure::{ParsedDocumentStructure, lower_document_structure},
};
use crate::text_safety::mask_terminal_controls;
use crate::tldr::{TldrPageLocation, TldrParseError, parse_tldr_page};

/// Complete result of parsing one `ManT`-flavoured Markdown input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMarkdown {
    /// Authoritative normalized Markdown document.
    pub document: Document,
    /// Optional document-owned quick reference.
    pub tldr: Option<TldrDocument>,
}

/// Invalid structure in `ManT`'s optional top-level Markdown extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownParseError {
    /// The top-level `ManT` tldr container is malformed.
    TldrDirective(TldrDirectiveError),
    /// Embedded tldr Markdown is structurally invalid.
    TldrPage(TldrParseError),
}

impl fmt::Display for MarkdownParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TldrDirective(error) => error.fmt(formatter),
            Self::TldrPage(error) => write!(formatter, "invalid embedded tldr page: {error}"),
        }
    }
}

impl Error for MarkdownParseError {}

/// Split `ManT`'s optional leading tldr preface from the Markdown document.
///
/// Invisible HTML comments delimit the preface so `CommonMark` renderers can
/// present the enclosed tldr-pages Markdown without leaking extension syntax.
/// It must be the first non-empty construct. The remaining source is parsed
/// independently, so its first H1 remains document-heading content rather than part
/// of the preface.
///
/// # Errors
///
/// Returns [`MarkdownParseError`] for an unterminated preface or malformed
/// embedded tldr page.
pub fn parse_markdown(
    source_text: &str,
    source_path: Option<String>,
) -> Result<ParsedMarkdown, MarkdownParseError> {
    let mut sanitize_diagnostics = Vec::new();
    let sanitized = sanitize_source(source_text, &mut sanitize_diagnostics);
    let source_text = sanitized.as_deref().unwrap_or(source_text);
    let parts = split_markdown(source_text).map_err(MarkdownParseError::TldrDirective)?;
    let tldr = parts
        .tldr
        .map(|source| {
            parse_tldr_page(
                source,
                TldrPageLocation {
                    platform: "embedded".to_owned(),
                    language: "und".to_owned(),
                    source_path: source_path.clone().unwrap_or_else(|| "<stdin>".to_owned()),
                },
            )
            .map(|mut page| {
                page.origin = TldrOrigin::Embedded;
                page
            })
            .map_err(MarkdownParseError::TldrPage)
        })
        .transpose()?;
    let mut entry_diagnostics = Vec::new();
    let prepared = PreparedMarkdown::new(parts.document.as_ref(), &mut entry_diagnostics);
    let mut document = parse_document_with_entries(prepared, source_path, &mut entry_diagnostics);
    if !entry_diagnostics.is_empty() {
        entry_diagnostics.extend(std::mem::take(&mut document.diagnostics));
        document.diagnostics = entry_diagnostics;
    }
    if !sanitize_diagnostics.is_empty() {
        sanitize_diagnostics.extend(std::mem::take(&mut document.diagnostics));
        document.diagnostics = sanitize_diagnostics;
    }
    Ok(ParsedMarkdown { document, tldr })
}

/// Mask a leading BOM and terminal-unsafe control characters with spaces.
///
/// A BOM would hide the tldr opening marker and demote the first heading, while
/// raw control characters would pass escape sequences through to terminals.
/// Replacements keep every byte offset valid for source coordinates.
fn sanitize_source(source_text: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<String> {
    let bom = source_text.starts_with('\u{feff}');
    let rest = if bom {
        &source_text['\u{feff}'.len_utf8()..]
    } else {
        source_text
    };
    let (masked, controls) = mask_terminal_controls(rest);
    if !bom && masked.is_none() {
        return None;
    }

    let mut sanitized = String::with_capacity(source_text.len());
    if bom {
        sanitized.push_str("   ");
    }
    sanitized.push_str(masked.as_deref().unwrap_or(rest));

    if bom {
        diagnostics.push(Diagnostic {
            impact: mant_ir::DiagnosticImpact::None,
            level: DiagnosticLevel::Warning,
            code: Some("markdown.byte-order-mark".to_owned()),
            message: "masked a leading byte-order mark".to_owned(),
            source: None,
        });
    }
    if controls > 0 {
        diagnostics.push(Diagnostic {
            impact: mant_ir::DiagnosticImpact::None,
            level: DiagnosticLevel::Warning,
            code: Some("markdown.control-characters".to_owned()),
            message: format!("masked {controls} terminal-unsafe control character(s)"),
            source: None,
        });
    }
    Some(sanitized)
}

/// Lower the ordinary document portion after extension extraction.
#[cfg(test)]
fn parse_document(source_text: &str, source_path: Option<String>) -> Document {
    let mut diagnostics = Vec::new();
    parse_document_with_entries(
        PreparedMarkdown {
            source: source_text,
            display_source: source_text.to_owned(),
            events: Parser::new_ext(source_text, markdown_options())
                .into_offset_iter()
                .collect(),
            declarations: directives::SemanticDeclarations::default(),
        },
        source_path,
        &mut diagnostics,
    )
}

fn parse_document_with_entries(
    prepared: PreparedMarkdown<'_>,
    source_path: Option<String>,
    entry_diagnostics: &mut Vec<Diagnostic>,
) -> Document {
    let PreparedMarkdown {
        source: source_text,
        display_source,
        events,
        mut declarations,
    } = prepared;
    let source = MarkdownSource::new(&display_source);
    let ParsedDocumentStructure {
        diagnostics,
        mut root_blocks,
        flat_sections,
        mut ids,
        document_title_id,
    } = lower_document_structure(events, &source);
    let mut sections = nest_sections(flat_sections);
    let extracted_title = extract_document_title(
        &mut root_blocks,
        &mut sections,
        document_title_id.as_deref(),
    );
    let mut document_fragment_aliases = Vec::new();
    let heading = extracted_title.map(|(heading, fragment_aliases)| {
        ids.remap_target(document_title_id.as_deref(), Some(DOCUMENT_ROOT_ID));
        document_fragment_aliases = fragment_aliases;
        heading
    });
    normalize_markdown_layout(
        &MarkdownSource::new(source_text),
        &mut root_blocks,
        &mut sections,
    );
    normalize_entry_lists(&mut root_blocks, &mut declarations, entry_diagnostics);
    normalize_section_entries(&mut sections, &mut declarations, entry_diagnostics);
    declarations.report_unattached(entry_diagnostics);
    let retained_targets = crate::definitions::identify_definitions(
        &mut root_blocks,
        &mut sections,
        // Link aliases are selectors, not physical anchors. Reserve only the
        // final section destinations so an unrelated heading alias cannot
        // perturb a semantic entry's inferred ID.
        &ids.reserved_targets(),
        source_path.as_deref(),
    );
    ids.retain_targets(retained_targets);
    let mut document = Document {
        parser: Some(markdown_parser()),
        source: DocumentSource {
            format: SourceFormat::Markdown,
            path: source_path,
        },
        meta: DocumentMeta::default(),
        heading,
        fragment_aliases: document_fragment_aliases,
        diagnostics,
        blocks: root_blocks,
        sections,
    };
    for (old, new) in metadata::apply(&mut document, declarations.bindings) {
        ids.replace_target(&old, new);
    }
    entry_diagnostics.extend(crate::producer_identity::outline_identity_diagnostics(
        &document.blocks,
        &document.sections,
        "markdown",
    ));
    ids.resolve_links(&mut document);
    document.diagnostics.extend(validate_document(&document));
    document
}

fn markdown_parser() -> ParserInfo {
    ParserInfo {
        name: "pulldown-cmark".to_owned(),
        version: "0.13".to_owned(),
    }
}

fn normalize_section_entries(
    sections: &mut [Section],
    declarations: &mut directives::SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for section in sections {
        normalize_entry_lists(&mut section.blocks, declarations, diagnostics);
        normalize_section_entries(&mut section.children, declarations, diagnostics);
    }
}
