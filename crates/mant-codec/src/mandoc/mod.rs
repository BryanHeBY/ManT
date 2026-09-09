//! Lowers the owned libmandoc syntax tree into `ManT`'s stable document model.

mod adjacency;
mod blocks;
mod containers;
mod controls;
mod declaration_groups;
mod diagnostics;
mod formatter;
pub(crate) mod inline;
mod layout;
mod navigation;
mod redirect;
mod reference;
mod roff_escape;
pub use redirect::{RedirectSyntaxError, redirect_target};
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
    Compression, Document as MandocDocument, IncludePolicy, MacroSet, Node, ParseError,
    ParseOptions, ParseReport, Parser,
};
use mant_ir::{
    Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource, ParserInfo, SourceFormat,
    SourceSpan, validate_document,
};

use self::{roff_escape::visible_text, source_lines::SourceLineIndex};
use crate::text_safety::mask_terminal_control_bytes;

const MAX_INLINE_EQUATION_NORMALIZATIONS: usize = 256;

/// Parse already prepared bytes; `path` is a source label, never opened.
/// Includes are disabled and compression must already have been decoded.
///
/// # Errors
/// Returns the native parser's controlled failure for invalid or bounded input.
pub fn parse_plain_manual(path: &Path, source: &[u8]) -> Result<Document, ParseError> {
    parse_plain_manual_report(path, source).map(|(document, _)| document)
}

/// Lower one owned witness from the same input, without filesystem fallbacks.
///
/// # Errors
/// Returns the native parser's controlled failure for invalid or bounded input.
pub fn parse_plain_manual_report(
    path: &Path,
    source: &[u8],
) -> Result<(Document, ParseReport), ParseError> {
    let (source, masked_controls) = mask_terminal_control_bytes(source);
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes(path, source.as_ref())?;
    let source_text = String::from_utf8_lossy(source.as_ref());
    let mut document = lower_mandoc_document_with_source(path, &report, Some(&source_text));
    if masked_controls > 0 {
        document.diagnostics.insert(
            0,
            Diagnostic {
                impact: mant_ir::DiagnosticImpact::None,
                level: DiagnosticLevel::Warning,
                code: Some("manual.control-characters".to_owned()),
                message: format!("masked {masked_controls} terminal-unsafe control character(s)"),
                source: None,
            },
        );
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
    diagnostics.extend(crate::producer_identity::outline_identity_diagnostics(
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
        heading: None,
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
