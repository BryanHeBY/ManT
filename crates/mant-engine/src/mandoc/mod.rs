//! Lowers the owned libmandoc syntax tree into `ManT`'s stable document model.

mod adjacency;
mod blocks;
mod containers;
mod declaration_groups;
mod diagnostics;
mod error;
mod formatter;
pub(crate) mod inline;
mod layout;
mod navigation;
mod reference;
mod roff_escape;
mod source;
mod source_context;
mod source_lines;
use source_context::{LoweringContext, TableTextBlock};
mod equations;
use equations::{EquationDelimiterChange, equation_delimiter_changes};
mod ast;
use ast::{first_part_children, part_child_groups, source_span};
mod targets;

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

use libmandoc_rs::{
    Compression, Document as MandocDocument, IncludePolicy, MacroSet, Node, ParseOptions,
    ParseReport, Parser,
};
use mant_ir::{
    Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource, ParserInfo, SourceFormat,
    SourceSpan, validate_document,
};

use self::{
    roff_escape::visible_text,
    source::{load_manual_source, redirect_target, resolve_manual_redirects},
    source_lines::SourceLineIndex,
};
use crate::ManualPage;
use crate::text_safety::mask_terminal_control_bytes;

pub use error::{ManualError, ManualErrorKind};
pub use source::MAX_MANUAL_BYTES;

const MAX_INLINE_EQUATION_NORMALIZATIONS: usize = 256;

/// Parse and normalize one standalone man or mdoc source file.
///
/// This safe convenience entry point does not expand `.so` redirects because
/// no caller-approved manual hierarchy accompanies a bare path. `ManT`'s indexed
/// query path uses [`parse_manual_page`] instead.
///
/// # Errors
///
/// Returns [`ManualError`] when the source cannot be opened, decoded, or parsed.
pub fn parse_manual_source(path: &Path) -> Result<Document, ManualError> {
    parse_manual_source_with_report(path).map(|(document, _)| document)
}

/// Parse through the production file pipeline, retaining its native witness.
///
/// Both results describe the same bounded, decompressed and control-masked
/// input, with the same deny-include policy as [`parse_manual_source`]. This
/// avoids a second parser invocation when consumers audit native-to-IR facts.
/// The native report is fully owned and no source file is read during lowering.
///
/// # Errors
///
/// Returns [`ManualError`] on source, decompression, redirect or parser failure.
pub fn parse_manual_source_with_report(
    path: &Path,
) -> Result<(Document, ParseReport), ManualError> {
    let loaded = load_manual_source(path)?;
    reject_standalone_redirect(path, &loaded.source)?;
    parse_plain_manual_report(path, &loaded.source, None)
}

/// Parse one already bounded, uncompressed standalone roff input.
///
/// This is the standard-input counterpart of [`parse_manual_source`]. It does
/// not expand `.so` redirects and never reads another file.
///
/// # Errors
///
/// Returns [`ManualError`] when libmandoc rejects the input.
pub fn parse_manual_bytes(path: &Path, source: &[u8]) -> Result<Document, ManualError> {
    reject_standalone_redirect(path, source)?;
    parse_plain_manual(path, source, None)
}

fn reject_standalone_redirect(path: &Path, source: &[u8]) -> Result<(), ManualError> {
    if redirect_target(path, source)?.is_some() {
        return Err(ManualError::redirect(
            path,
            "standalone .so redirects require MANPATH discovery and cannot be followed by --input",
        ));
    }
    Ok(())
}

/// Parse an indexed manual, resolving `.so` redirects against its discovered
/// manual hierarchy without falling back to the process working directory.
///
/// # Errors
///
/// Returns [`ManualError`] when the source cannot be opened, decoded, or parsed.
pub fn parse_manual_page(page: &ManualPage) -> Result<Document, ManualError> {
    let resolved = resolve_manual_redirects(page)?;
    parse_plain_manual(
        &page.path,
        &resolved.source,
        resolved.alias_target.as_deref(),
    )
}

fn parse_plain_manual(
    path: &Path,
    source: &[u8],
    alias_target: Option<&str>,
) -> Result<Document, ManualError> {
    parse_plain_manual_report(path, source, alias_target).map(|(document, _)| document)
}

fn parse_plain_manual_report(
    path: &Path,
    source: &[u8],
    alias_target: Option<&str>,
) -> Result<(Document, ParseReport), ManualError> {
    let (source, masked_controls) = mask_terminal_control_bytes(source);
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes(path, source.as_ref())
    .map_err(ManualError::from)?;
    let source_text = String::from_utf8_lossy(source.as_ref());
    let mut document = lower_mandoc_document_with_source(path, &report, Some(&source_text));
    if masked_controls > 0 {
        document.diagnostics.insert(
            0,
            Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some("manual.control-characters".to_owned()),
                message: format!("masked {masked_controls} terminal-unsafe control character(s)"),
                source: None,
            },
        );
    }
    if let Some(alias_target) = alias_target {
        document.meta.alias_target = Some(alias_target.to_owned());
    }
    Ok((document, report))
}

/// Convert a completed low-level parse into the stable document contract.
#[must_use]
pub fn lower_mandoc_document(path: &Path, report: &ParseReport) -> Document {
    lower_mandoc_document_with_source(path, report, None)
}

fn lower_mandoc_document_with_source(
    path: &Path,
    report: &ParseReport,
    source: Option<&str>,
) -> Document {
    let parsed: &MandocDocument = &report.document;
    let target_plan = targets::NativeTargetPlan::build(&parsed.root);
    let explicit_targets = target_plan.explicit();
    let mut context = LoweringContext::new(parsed.metadata.name.as_deref(), source);
    context.macro_set = parsed.macro_set;
    declaration_groups::record(&parsed.root, &mut context.native_heads.borrow_mut());
    context.reserve_section_ids(explicit_targets);
    let mut diagnostics = diagnostics::lower_diagnostics(&report.diagnostics);
    let mut sections = blocks::lower_sections(&parsed.root, &mut context);
    let mut root_blocks = blocks::lower_root_blocks(&parsed.root, &context);
    diagnostics.extend(context.take_diagnostics());
    navigation::normalize_generated_anchors(&mut root_blocks, &mut sections, explicit_targets);
    let mut retained_targets = navigation::native_anchor_ids(&root_blocks, &sections);
    retained_targets.extend(explicit_targets.iter().cloned());
    retained_targets.extend(crate::definitions::identify_definitions_with_evidence(
        &mut root_blocks,
        &mut sections,
        explicit_targets,
        parsed.metadata.name.as_deref(),
        &context.native_heads.borrow(),
    ));
    diagnostics.extend(crate::selectors::semantic_selector_diagnostics(
        &root_blocks,
        &sections,
        "manual",
    ));
    diagnostics.extend(crate::definitions::manual_discovery_diagnostics(&sections));
    navigation::resolve_navigation(
        &mut root_blocks,
        &mut sections,
        &retained_targets,
        &mut diagnostics,
    );
    let mut document = Document {
        parser: Some(ParserInfo {
            name: "libmandoc".to_owned(),
            version: libmandoc_rs::LIBMANDOC_VERSION.to_owned(),
        }),
        source: DocumentSource {
            format: match parsed.macro_set {
                MacroSet::Mdoc => SourceFormat::Mdoc,
                MacroSet::Man | MacroSet::None => SourceFormat::Man,
            },
            path: Some(path.to_string_lossy().into_owned()),
        },
        meta: DocumentMeta {
            title: normalize_metadata(parsed.metadata.title.as_deref()),
            manual_section: normalize_metadata(parsed.metadata.section.as_deref()),
            date: normalize_metadata(parsed.metadata.date.as_deref()),
            volume: normalize_metadata(parsed.metadata.volume.as_deref()),
            os: normalize_metadata(parsed.metadata.os.as_deref()),
            arch: normalize_metadata(parsed.metadata.arch.as_deref()),
            names: normalize_metadata(parsed.metadata.name.as_deref())
                .into_iter()
                .collect(),
            alias_target: parsed.metadata.alias_target.clone(),
        },
        fragment_aliases: Vec::new(),
        diagnostics,
        blocks: root_blocks,
        sections,
    };
    document.diagnostics.extend(validate_document(&document));
    document
}

/// Metadata strings come from roff macro arguments rather than visible text
/// nodes, so libmandoc can legitimately retain zero-width escapes such as
/// `\&`. Normalize them through the same inline decoder used for document
/// content before exposing the renderer-neutral contract.
fn normalize_metadata(value: Option<&str>) -> Option<String> {
    value.map(visible_text)
}

#[cfg(test)]
mod tests;
