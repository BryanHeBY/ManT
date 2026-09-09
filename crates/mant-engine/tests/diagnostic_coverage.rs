//! Semantic export consumes public producer coverage, not parser code names.
use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_engine::query_markdown_text;
use mant_ir::{Diagnostic, DiagnosticImpact, DiagnosticLevel};

#[test]
fn custom_coverage_failure_disables_annotations_without_changing_plain_markdown() {
    let source =
        "# Tool\n\n<!-- mant:entries role=command case=sensitive -->\n- `probe`: Inspect data.\n";
    for impact in [DiagnosticImpact::None, DiagnosticImpact::SemanticCoverage] {
        let mut content = query_markdown_text(source, None).unwrap();
        let plain = render_markdown_with_options(&content, MarkdownOptions::default());
        content
            .document
            .as_mut()
            .unwrap()
            .diagnostics
            .push(Diagnostic {
                impact,
                level: DiagnosticLevel::Style,
                code: Some("another-parser.coverage".into()),
                message: "a semantic declaration was rejected".into(),
                source: None,
            });
        let exported = render_markdown_with_options(
            &content,
            MarkdownOptions {
                preserve_semantics: true,
                ..MarkdownOptions::default()
            },
        );
        assert_eq!(
            exported.contains("mant:entries"),
            impact == DiagnosticImpact::None
        );
        assert_eq!(
            render_markdown_with_options(&content, MarkdownOptions::default()),
            plain
        );
        if impact == DiagnosticImpact::SemanticCoverage {
            assert_eq!(exported, plain);
        }
    }
}
