//! Authored native diagnostics and annotation-coverage conversion.

use super::{
    AnnotatedDocument, AnnotatedProjectionError, AnnotationCheckState, AnnotationDimension,
    AnnotationIssueReason, AnnotationScope, CoverageScope, Diagnostic, DiagnosticImpact,
    DiagnosticLevel, KeyMap, Result, SourceSpan, key_source,
};

pub(super) fn native_diagnostics(page: &AnnotatedDocument) -> Result<Vec<Diagnostic>> {
    page.diagnostics
        .iter()
        .map(|native| {
            let source = if native.span == 0 {
                None
            } else {
                let span = page.spans.get((native.span - 1) as usize).ok_or(
                    AnnotatedProjectionError::Relation("diagnostic span missing"),
                )?;
                span.line_column
                    .map(
                        |(line, column, end_line, end_column)| -> Result<SourceSpan> {
                            Ok(SourceSpan {
                                source: key_source(span.source)?,
                                byte_range: None,
                                line,
                                column,
                                end_line: (end_line != 0).then_some(end_line),
                                end_column: (end_column != 0).then_some(end_column),
                            })
                        },
                    )
                    .transpose()?
            };
            Ok(Diagnostic {
                level: match native.level {
                    1 => DiagnosticLevel::Style,
                    2 => DiagnosticLevel::Warning,
                    3 => DiagnosticLevel::Error,
                    4 => DiagnosticLevel::Unsupported,
                    _ => {
                        return Err(AnnotatedProjectionError::Relation(
                            "unknown native diagnostic level",
                        ));
                    }
                },
                impact: DiagnosticImpact::None,
                code: Some(format!("mandoc.native-{}", native.code)),
                message: native.message.clone(),
                source,
                coverage_scope: None,
            })
        })
        .collect()
}

pub(super) fn coverage_diagnostics(
    page: &AnnotatedDocument,
    keys: &KeyMap,
) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    for check in &page.coverage.checks {
        if check.state == AnnotationCheckState::Pending {
            diagnostics.push(coverage_diagnostic(
                check.dimension,
                "unverified",
                CoverageScope::Document,
                None,
            ));
        }
    }
    for issue in &page.coverage.issues {
        let scope = match issue.scope {
            AnnotationScope::Document => CoverageScope::Document,
            AnnotationScope::Section(global) => CoverageScope::Section {
                key: keys.required(global, 1)?,
            },
            AnnotationScope::Owner(global) => CoverageScope::Owner {
                key: keys.required(global, 2)?,
            },
            AnnotationScope::Region(global) => CoverageScope::Region {
                key: keys.required(global, 5)?,
            },
            AnnotationScope::Source(raw) => CoverageScope::Source {
                key: key_source(raw)?,
            },
        };
        let source = issue
            .source
            .map(|position| -> Result<SourceSpan> {
                Ok(SourceSpan {
                    source: key_source(position.source)?,
                    byte_range: None,
                    line: position.line,
                    column: position.column,
                    end_line: None,
                    end_column: None,
                })
            })
            .transpose()?;
        let reason = match issue.reason {
            AnnotationIssueReason::NotObserved => "not-observed",
            AnnotationIssueReason::Unverified => "unverified",
            AnnotationIssueReason::Rejected => "rejected",
            AnnotationIssueReason::AmbiguousSurvival => "ambiguous-survival",
        };
        diagnostics.push(coverage_diagnostic(issue.dimension, reason, scope, source));
    }
    Ok(diagnostics)
}

fn coverage_diagnostic(
    dimension: AnnotationDimension,
    reason: &str,
    coverage_scope: CoverageScope,
    source: Option<SourceSpan>,
) -> Diagnostic {
    let dimension = match dimension {
        AnnotationDimension::Section => "section",
        AnnotationDimension::OwnerBoundary => "owner-boundary",
        AnnotationDimension::Declaration => "declaration",
        AnnotationDimension::Link => "link",
        AnnotationDimension::Anchor => "anchor",
        AnnotationDimension::Relation => "relation",
        AnnotationDimension::Source => "source",
        AnnotationDimension::Join => "join",
    };
    Diagnostic {
        level: DiagnosticLevel::Unsupported,
        impact: DiagnosticImpact::SemanticCoverage,
        code: Some(format!("annotated.coverage.{dimension}.{reason}")),
        message: format!("{dimension} evidence {reason}"),
        source,
        coverage_scope: Some(coverage_scope),
    }
}
