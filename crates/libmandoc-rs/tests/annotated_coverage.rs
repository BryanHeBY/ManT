#![cfg(feature = "annotated")]

//! R01 coverage is explicit and conservative, not a whole-page completion bit.
//! The independent native evidence census is a later unit; this fixture must
//! not treat the current document-wide fallback count as a protocol invariant.

use libmandoc_rs::annotated::{
    AnnotatedRenderer, AnnotationCheckState, AnnotationDimension, AnnotationIssueReason,
    AnnotationProducer, AnnotationScope,
};
use libmandoc_rs::{InputFormat, SourceBundle};

#[test]
fn native_coverage_records_document_wide_unverified_dimensions() {
    // The exact input was run first through the pinned CVS reference with
    // -Tutf8 -Owidth=78.  man_term.c::print_man_node emits the heading and
    // .MR instance, but R01 has not proved their final semantic ranges.
    let mut bundle = SourceBundle::new();
    bundle
        .insert("a.1", b".TH A 1\n.SH NAME\nA\n.MR printf 3\n".to_vec())
        .unwrap();
    let page = AnnotatedRenderer::default()
        .render_bundle("a.1", &bundle, InputFormat::Man)
        .unwrap();
    let coverage = &page.coverage;
    assert_eq!(coverage.checks.len(), 24);
    assert!(
        coverage
            .checks
            .iter()
            .all(|check| check.state != AnnotationCheckState::Checked)
    );
    assert!(coverage.checks.iter().any(|check| {
        check.producer == AnnotationProducer::Native
            && check.dimension == AnnotationDimension::Declaration
            && check.state == AnnotationCheckState::NotApplicable
    }));
    assert!(coverage.checks.iter().any(|check| {
        check.producer == AnnotationProducer::Codec
            && check.dimension == AnnotationDimension::Declaration
            && check.state == AnnotationCheckState::Pending
    }));
    assert!(coverage.checks.iter().any(|check| {
        check.producer == AnnotationProducer::Validator
            && check.dimension == AnnotationDimension::Join
            && check.state == AnnotationCheckState::Pending
    }));
    assert!(coverage.issues.iter().any(|issue| {
        issue.dimension == AnnotationDimension::Section
            && issue.reason == AnnotationIssueReason::Unverified
    }));
    for check in &coverage.checks {
        let has_issue = coverage
            .issues
            .iter()
            .any(|issue| issue.producer == check.producer && issue.dimension == check.dimension);
        assert_eq!(has_issue, check.state == AnnotationCheckState::Unverified);
    }
    assert!(
        coverage
            .issues
            .iter()
            .all(|issue| { issue.scope == AnnotationScope::Document && issue.source.is_none() })
    );
}
