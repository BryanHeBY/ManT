//! Lowers the owned libmandoc syntax tree into `ManT`'s stable document model.

mod adjacency;
mod blocks;
mod containers;
mod controls;
mod declaration_groups;
mod diagnostics;
mod escape_coverage;
mod formatter;
pub(crate) mod inline;
mod layout;
mod link_layout;
pub use layout::roff_vertical_space_lines;
mod literal_reading;
mod navigation;
mod owner_layout;
mod redirect;
mod reference;
mod roff_escape;
pub use redirect::{RedirectSyntaxError, redirect_target};
pub use roff_escape::{RoffWordEffect, RoffWordEvidence};
mod source_context;
mod source_lines;
mod table_layout_budget;
mod table_recovery_budget;
use source_context::{LoweringContext, TableTextBlock};
mod ast;
mod equation_wire;
mod equations;
use ast::{first_part_children, part_child_groups, source_span};
mod targets;

use std::{cell::RefCell, collections::HashSet, path::Path};

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

pub use section_identity::{
    RoffSectionHeading, RoffSectionTargets, SectionLabelEvidence,
    authored_section_phrase as observe_roff_section_phrase,
    section_label_evidence as observe_roff_section_label_evidence,
    section_label_has_glyphs as observe_roff_section_label_glyphs,
};
mod section_identity;

/// Observe the exact identity of an already parsed roff link operand.
/// An empty string is a proven empty identity. `None` means that recovery
/// or an execution limit prevented an exact observation.
#[doc(hidden)]
#[must_use]
pub fn observe_roff_link_operand(source: &str) -> Option<String> {
    let (identity, exact) = inline::link_identity_with_evidence(source);
    exact.then_some(identity)
}

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
    parse_with_reading_policy(path, source, InitialCodeRow::OmitSingle, None)
}

/// Decode plain roff and derive native link membership for the same snapshot.
/// The returned document keeps exactly the legacy serialization.
/// # Errors
/// Returns the same bounded native parse errors as the plain decoder.
pub fn parse_plain_manual_with_link_layout(
    path: &Path,
    source: &[u8],
) -> Result<crate::DecodedLinkDocument, ParseError> {
    let mut layout = mant_ir::LinkLayout::default();
    let (document, _) =
        parse_with_reading_policy(path, source, InitialCodeRow::OmitSingle, Some(&mut layout))?;
    Ok(crate::DecodedLinkDocument::from_decode(document, layout))
}

#[derive(Clone, Copy)]
enum InitialCodeRow {
    OmitSingle,
    #[cfg(test)]
    RetainExecuted,
}

// Native execution fixtures retain their independent pre-reading oracle.
// Public codec entry points always select the documented reading projection.
#[cfg(test)]
pub(crate) fn parse_native_manual(path: &Path, source: &[u8]) -> Result<Document, ParseError> {
    parse_with_reading_policy(path, source, InitialCodeRow::RetainExecuted, None)
        .map(|(document, _)| document)
}

fn parse_with_reading_policy(
    path: &Path,
    source: &[u8],
    initial_code_row: InitialCodeRow,
    link_layout: Option<&mut mant_ir::LinkLayout>,
) -> Result<(Document, ParseReport), ParseError> {
    let (source, masked_controls) = mask_terminal_control_bytes(source);
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes(path, source.as_ref())?;
    let source_text = String::from_utf8_lossy(source.as_ref());
    let mut document = lower_mandoc_document_with_source(
        path,
        &report,
        Some(&source_text),
        initial_code_row,
        link_layout,
    );
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
    lower_mandoc_document_with_source(path, report, None, InitialCodeRow::OmitSingle, None)
}

fn lower_mandoc_document_with_source(
    path: &Path,
    report: &ParseReport,
    source: Option<&str>,
    initial_code_row: InitialCodeRow,
    link_layout: Option<&mut mant_ir::LinkLayout>,
) -> Document {
    let parsed: &MandocDocument = &report.document;
    let mut context = LoweringContext::new(parsed.metadata.name.as_deref(), source);
    context
        .annotations
        .capture_link_layout(link_layout.is_some());
    let target_plan =
        targets::NativeTargetPlan::build(&parsed.root, &mut context.authored_section_targets);
    let explicit_targets = target_plan.explicit();
    context.macro_set = parsed.macro_set;
    // mdoc_macro.c closes the Bl/It scopes at .El. A later indented
    // block at the same structural level belongs to their parent.
    context.native_heads.get_mut().explicit_list_owners = parsed.macro_set == MacroSet::Mdoc;
    context
        .native_target_owners
        .clone_from(target_plan.validated_owners());
    context.scope_posts.index_structural_payload(&parsed.root);
    declaration_groups::record(&parsed.root, &mut context.native_heads.borrow_mut());
    context.reserve_section_ids(explicit_targets);
    let mut diagnostics = diagnostics::lower_diagnostics(&report.diagnostics);
    diagnostics.extend(equations::diagnostics(&parsed.root));
    let (mut root_blocks, mut sections) =
        blocks::lower_document_structure(&parsed.root, &mut context);
    let publication_diagnostics = context.annotations.finish(
        &mut root_blocks,
        &mut sections,
        &mut context.native_heads.borrow_mut(),
        |nodes| {
            navigation::publish_inline_navigation(
                nodes,
                &context.authored_section_targets,
                &mut diagnostics,
            )
        },
    );
    diagnostics.extend(publication_diagnostics);
    diagnostics.extend(context.annotations.take_diagnostics());
    diagnostics.extend(context.take_diagnostics());
    literal_reading::normalize(
        &mut root_blocks,
        &mut sections,
        &mut context.native_heads.borrow_mut(),
        initial_code_row,
    );
    navigation::normalize_generated_anchors(&mut root_blocks, &mut sections, explicit_targets);
    let identified = crate::definitions::identify_definitions_with_evidence(
        &mut root_blocks,
        &mut sections,
        explicit_targets,
        parsed.metadata.name.as_deref(),
        &context.native_heads.borrow(),
    );
    diagnostics.extend(identified.diagnostics);
    diagnostics.extend(crate::producer_identity::outline_identity_diagnostics(
        &root_blocks,
        &sections,
        "manual",
    ));
    diagnostics.extend(crate::definitions::manual_discovery_diagnostics(&sections));
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
    equation_wire::bound_wire_structure(&mut document);
    owner_layout::bound_layout_hints(&mut document);
    if let Some(layout) = link_layout {
        *layout = link_layout::seal(&mut document);
    }
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
