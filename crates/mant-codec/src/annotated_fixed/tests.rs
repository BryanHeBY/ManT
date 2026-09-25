use libmandoc_rs::annotated::{
    AnnotatedDocument, AnnotatedMark, AnnotatedMetadata, AnnotatedRenderer, AnnotationCoverage,
    AnnotationCoverageIssue, AnnotationDimension, AnnotationIssueReason, AnnotationProducer,
    AnnotationScope,
};
use libmandoc_rs::{InputFormat, SourceBundle};
use mant_ir::{
    DisplayRole, DocumentAddress, DocumentBody, DocumentIndex, EntryKind, LinkTarget,
    MarkdownOrigin, OwnerHeadRole, OwnerRole, ParameterKind, SourceKey, validate_document,
};
use mant_protocol::{
    DocumentScope, EvidenceBasis, EvidenceClass, ExplanationOptions, ExplanationQuery,
    ResolvedDocumentScope, ScopedDocument, SearchCase, SearchQuery, SearchScope, SearchSyntax,
};

use super::{isolation, lower_annotated_document, project_annotated_manual};

mod declarations;
mod evidence;
mod marks;
mod reading;
mod reading_groups;
mod targets;

fn bundle(input: &[u8]) -> SourceBundle {
    let mut bundle = SourceBundle::new();
    bundle.insert("t.1", input.to_vec()).unwrap();
    bundle
}

fn native_query(input: &[u8], width: u32) -> mant_ir::ResolvedContent {
    let page = AnnotatedRenderer::new(width)
        .unwrap()
        .render_bundle("t.1", &bundle(input), InputFormat::Man)
        .unwrap();
    mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(lower_annotated_document(page).unwrap()),
        tldr: None,
    }
}

fn visible_total(query: &mant_ir::ResolvedContent, pattern: &str) -> u32 {
    mant_query::search_query(
        query,
        &SearchQuery {
            pattern: pattern.to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap()
    .total
}

fn assert_single_section_covers_each_visible_byte_once(fixed: &mant_ir::FixedBody) {
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    assert_eq!(reader.roots().len(), 1);
    let parts = reader.subtree_parts(reader.roots()[0]).unwrap();
    let mut counts = fixed
        .surface
        .runs
        .iter()
        .map(|run| vec![0_u8; usize::try_from(run.byte_count).unwrap()])
        .collect::<Vec<_>>();
    for part in parts {
        let slots = &mut counts[(part.slice.run.get() - 1) as usize];
        for count in &mut slots[usize::try_from(part.slice.start_byte).unwrap()
            ..usize::try_from(part.slice.end_byte).unwrap()]
        {
            *count += 1;
        }
    }
    for (run, counts) in fixed.surface.runs.iter().zip(counts) {
        if run.label.role != DisplayRole::Layout {
            assert!(
                counts.iter().all(|&count| count == 1),
                "run {run:?}: {counts:?}"
            );
        }
    }
}
