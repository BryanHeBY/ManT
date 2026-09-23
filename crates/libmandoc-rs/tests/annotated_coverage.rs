#![cfg(feature = "annotated")]

//! R01 coverage is explicit and conservative, not a whole-page completion bit.
//! The final-AST presence census can prove absence, not complete extraction;
//! this fixture does not treat a fallback issue count as a protocol invariant.

use libmandoc_rs::annotated::{
    AnnotatedRenderer, AnnotationCheckState, AnnotationDimension, AnnotationIssueReason,
    AnnotationProducer, AnnotationScope,
};
use libmandoc_rs::{InputFormat, SourceBundle};

fn render(input: &[u8]) -> libmandoc_rs::annotated::AnnotatedDocument {
    let mut bundle = SourceBundle::new();
    bundle.insert("a.1", input.to_vec()).unwrap();
    AnnotatedRenderer::default()
        .render_bundle("a.1", &bundle, InputFormat::Man)
        .unwrap()
}

fn native_state(
    page: &libmandoc_rs::annotated::AnnotatedDocument,
    dimension: AnnotationDimension,
) -> AnnotationCheckState {
    page.coverage
        .checks
        .iter()
        .find(|check| check.producer == AnnotationProducer::Native && check.dimension == dimension)
        .unwrap()
        .state
}

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

#[test]
fn absent_final_ast_candidates_are_not_applicable() {
    // Pinned CVS -Tutf8 -Owidth=78 and -Ttree were run first on this exact
    // input. read.c::mparse_result() validates and postprocesses tags before
    // the final AST walk; no heading, link, or NODE_ID survives here.
    let page = render(b".TH T 1\n.PP\nbody\n");
    for dimension in [
        AnnotationDimension::Section,
        AnnotationDimension::Link,
        AnnotationDimension::Anchor,
    ] {
        assert_eq!(
            native_state(&page, dimension),
            AnnotationCheckState::NotApplicable
        );
        assert!(!page.coverage.issues.iter().any(|issue| {
            issue.producer == AnnotationProducer::Native && issue.dimension == dimension
        }));
    }
    assert_eq!(
        native_state(&page, AnnotationDimension::Source),
        AnnotationCheckState::Unverified
    );
}

#[test]
fn parsed_link_candidate_without_mark_is_not_observed_not_absent() {
    // Pinned CVS -Tutf8 -Owidth=78 and -Ttree were run first on this exact
    // input: man_term.c traverses a .UR block with an empty head, while
    // collector push_node() intentionally creates no HTML-style link mark.
    let page = render(b".TH T 1\n.SH D\n.UR\n.UE\n");
    assert_eq!(
        native_state(&page, AnnotationDimension::Link),
        AnnotationCheckState::Unverified
    );
    assert!(page.coverage.issues.iter().any(|issue| {
        issue.producer == AnnotationProducer::Native
            && issue.dimension == AnnotationDimension::Link
            && issue.reason == AnnotationIssueReason::NotObserved
            && issue.scope == AnnotationScope::Document
    }));
}

#[test]
fn native_link_coverage_keeps_unimplemented_mdoc_html_link_candidates() {
    // Each exact input first ran through the pinned CVS reference using
    // -Thtml and -Tutf8 -Owidth=78.  mdoc_html.c::mdoc_in_pre() and
    // mdoc_fd_pre()/mdoc_rs_pre() emit TAG_A, while this R01 collector has no
    // corresponding native LinkMark. Missing marks must not turn Link into N/A.
    for input in [
        b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh SYNOPSIS\n.In stdio.h\n".as_slice(),
        b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh SYNOPSIS\n.Fd #include <stdio.h>\n"
            .as_slice(),
        b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh REFERENCES\n.Rs\n.%U https://example.test\n.Re\n"
            .as_slice(),
        b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh REFERENCES\n.Rs\n.%R RFC 42\n.Re\n"
            .as_slice(),
    ] {
        let mut bundle = SourceBundle::new();
        bundle.insert("a.1", input.to_vec()).unwrap();
        let page = AnnotatedRenderer::default()
            .render_bundle("a.1", &bundle, InputFormat::Mdoc)
            .unwrap();
        assert_eq!(
            native_state(&page, AnnotationDimension::Link),
            AnnotationCheckState::Unverified
        );
        assert!(page.coverage.issues.iter().any(|issue| {
            issue.producer == AnnotationProducer::Native
                && issue.dimension == AnnotationDimension::Link
                && issue.reason == AnnotationIssueReason::NotObserved
                && issue.scope == AnnotationScope::Document
        }));
    }
}
