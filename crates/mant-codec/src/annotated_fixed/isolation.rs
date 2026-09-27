//! Safe native display survives rejection of optional mark relationships.
//!
//! This path does not re-run the roff formatter or infer substitute owners.
//! It accepts only a fully checked byte arena, rows, runs and source table;
//! every semantic label is removed so no stale entry or link remains active.

use super::{
    AnnotatedDocument, AnnotatedProjectionError, CoverageScope, Diagnostic, DiagnosticImpact,
    DiagnosticLevel, DisplayLabel, DisplayRole, DisplayRun, DisplayStyle, DisplaySurface, Document,
    DocumentBody, FixedBody, ParserInfo, Result, key, key_source, metadata, native_diagnostics,
    rows, source_key, sources, validate_document, validate_document_sources,
};

pub(super) fn rejected_annotation(reason: &str, scope: CoverageScope) -> Diagnostic {
    Diagnostic {
        level: DiagnosticLevel::Unsupported,
        impact: DiagnosticImpact::SemanticCoverage,
        code: Some("annotated.internal-annotation-rejected".to_owned()),
        message: format!("native annotation relation rejected: {reason}"),
        source: None,
        source_key: None,
        coverage_scope: Some(scope),
    }
}

pub(super) fn body_only_document(mut page: AnnotatedDocument, reason: &str) -> Result<Document> {
    let sources = sources(&page)?;
    let mut row_keys = vec![0_u32; page.runs.len()];
    let rows = rows(&page, &mut row_keys)?;
    let mut runs = Vec::with_capacity(page.runs.len());
    for (index, run) in page.runs.iter().enumerate() {
        runs.push(DisplayRun {
            key: key(run.key, "native run has zero key")?,
            row: key(row_keys[index], "native run has no final row")?,
            column: run.column,
            width: run.width,
            byte_start: run.byte_start,
            byte_count: run.byte_count,
            label: DisplayLabel {
                owner: None,
                link: None,
                source: source_key(run.label.source)?,
                style: DisplayStyle {
                    bold: run.label.style & 1 != 0,
                    underline: run.label.style & 8 != 0,
                },
                role: match run.label.role {
                    1 => DisplayRole::Body,
                    4 => DisplayRole::DirectDraw,
                    5 => DisplayRole::Layout,
                    _ => {
                        return Err(AnnotatedProjectionError::Relation("unknown display role"));
                    }
                },
            },
        });
    }
    let mut diagnostics = native_diagnostics(&page)?;
    let surface = DisplaySurface {
        text: std::mem::take(&mut page.text),
        rows,
        runs,
    };
    surface.validate().map_err(|error| {
        AnnotatedProjectionError::RelationDetail(format!("invalid native display surface: {error}"))
    })?;
    diagnostics.push(rejected_annotation(reason, CoverageScope::Document));
    let document = Document {
        parser: Some(ParserInfo {
            name: "libmandoc-annotated".to_owned(),
            version: libmandoc_rs::LIBMANDOC_VERSION.to_owned(),
        }),
        sources,
        root_source: key_source(page.root_source)?,
        body: DocumentBody::Fixed(FixedBody {
            surface,
            root_configuration_hint: false,
            headings: Vec::new(),
            owners: Vec::new(),
            links: Vec::new(),
            anchors: Vec::new(),
            regions: Vec::new(),
        }),
        meta: metadata(&page),
        fragment_aliases: Vec::new(),
        diagnostics,
    };
    validate_document_sources(&document)
        .map_err(|_| AnnotatedProjectionError::Relation("invalid native source table"))?;
    if let Some(first) = validate_document(&document).into_iter().next() {
        return Err(AnnotatedProjectionError::RelationDetail(format!(
            "invalid native display document: {}: {}",
            first.code.as_deref().unwrap_or("unnamed"),
            first.message
        )));
    }
    Ok(document)
}

pub(super) fn strip_invalid_annotations(fixed: &mut FixedBody) {
    for run in &mut fixed.surface.runs {
        run.label.owner = None;
        run.label.link = None;
    }
    fixed.headings.clear();
    fixed.owners.clear();
    fixed.links.clear();
    fixed.anchors.clear();
    fixed.regions.clear();
}

/// Retract only invalid optional facts; valid sibling owners and inert link
/// labels remain available for reading against the unchanged native surface.
pub(super) fn isolate_optional_facts(fixed: &mut FixedBody) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for key in fixed.invalid_entry_keys() {
        fixed.owners[(key.get() - 1) as usize].entry = None;
        diagnostics.push(rejected_annotation(
            "entry facts did not close against the native head",
            CoverageScope::Owner { key },
        ));
    }
    for key in fixed.invalid_link_target_keys() {
        fixed.links[(key.get() - 1) as usize].target = None;
        diagnostics.push(rejected_annotation(
            "link target is invalid; activation disabled",
            CoverageScope::Document,
        ));
    }
    diagnostics
}
