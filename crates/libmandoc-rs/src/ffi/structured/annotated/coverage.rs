//! Closed native annotation-coverage table and scoped issue transfer.

use super::{
    AnnotatedError, AnnotatedMark, AnnotatedSource, AnnotationCheckState, AnnotationCoverage,
    AnnotationCoverageCheck, AnnotationCoverageIssue, AnnotationDimension, AnnotationIssueReason,
    AnnotationProducer, AnnotationScope, AnnotationSourcePosition, CoverageCheckView,
    CoverageIssueView, invalid_result, reserve,
};

fn coverage_producer(value: u32) -> Result<AnnotationProducer, AnnotatedError> {
    match value {
        1 => Ok(AnnotationProducer::Native),
        2 => Ok(AnnotationProducer::Codec),
        3 => Ok(AnnotationProducer::Validator),
        _ => Err(invalid_result()),
    }
}

fn coverage_dimension(value: u32) -> Result<AnnotationDimension, AnnotatedError> {
    match value {
        1 => Ok(AnnotationDimension::Section),
        2 => Ok(AnnotationDimension::OwnerBoundary),
        3 => Ok(AnnotationDimension::Declaration),
        4 => Ok(AnnotationDimension::Link),
        5 => Ok(AnnotationDimension::Anchor),
        6 => Ok(AnnotationDimension::Relation),
        7 => Ok(AnnotationDimension::Source),
        8 => Ok(AnnotationDimension::Join),
        _ => Err(invalid_result()),
    }
}

fn coverage_state(value: u32) -> Result<AnnotationCheckState, AnnotatedError> {
    match value {
        1 => Ok(AnnotationCheckState::Checked),
        2 => Ok(AnnotationCheckState::NotApplicable),
        3 => Ok(AnnotationCheckState::Unverified),
        4 => Ok(AnnotationCheckState::Pending),
        _ => Err(invalid_result()),
    }
}

fn coverage_reason(value: u32) -> Result<AnnotationIssueReason, AnnotatedError> {
    match value {
        1 => Ok(AnnotationIssueReason::NotObserved),
        2 => Ok(AnnotationIssueReason::Unverified),
        3 => Ok(AnnotationIssueReason::Rejected),
        4 => Ok(AnnotationIssueReason::AmbiguousSurvival),
        _ => Err(invalid_result()),
    }
}

fn coverage_issue_location(
    issue: &CoverageIssueView,
    marks: &[AnnotatedMark],
    sources: &[AnnotatedSource],
) -> Result<(AnnotationScope, Option<AnnotationSourcePosition>), AnnotatedError> {
    let scope = match issue.scope {
        1 if issue.scope_key == 0 => AnnotationScope::Document,
        2..=4 => {
            let mark = issue
                .scope_key
                .checked_sub(1)
                .and_then(|index| marks.get(index as usize))
                .ok_or_else(invalid_result)?;
            // Scope tags are 2/3/4; native heading/owner/region kinds are 1/2/5.
            let expected = match issue.scope {
                2 => 1,
                3 => 2,
                _ => 5,
            };
            if mark.key != issue.scope_key || mark.kind != expected {
                return Err(invalid_result());
            }
            match issue.scope {
                2 => AnnotationScope::Section(issue.scope_key),
                3 => AnnotationScope::Owner(issue.scope_key),
                _ => AnnotationScope::Region(issue.scope_key),
            }
        }
        5 if issue.scope_key != 0 && (issue.scope_key as usize) <= sources.len() => {
            AnnotationScope::Source(issue.scope_key)
        }
        _ => return Err(invalid_result()),
    };
    let source = if issue.source == 0 && issue.line == 0 && issue.column == 0 {
        None
    } else if issue.source != 0
        && (issue.source as usize) <= sources.len()
        && issue.line != 0
        && issue.column != 0
    {
        Some(AnnotationSourcePosition {
            source: issue.source,
            line: issue.line,
            column: issue.column,
        })
    } else {
        return Err(invalid_result());
    };
    if let (AnnotationScope::Source(scope_key), Some(position)) = (scope, source)
        && scope_key != position.source
    {
        return Err(invalid_result());
    }
    Ok((scope, source))
}

pub(super) fn transfer_coverage(
    checks: &[CoverageCheckView],
    issues: &[CoverageIssueView],
    marks: &[AnnotatedMark],
    sources: &[AnnotatedSource],
) -> Result<AnnotationCoverage, AnnotatedError> {
    if checks.len() != 24 {
        return Err(invalid_result());
    }
    let mut states = [[None; 8]; 3];
    let mut owned_checks = reserve(checks.len())?;
    for check in checks {
        let producer = coverage_producer(check.producer)?;
        let dimension = coverage_dimension(check.dimension)?;
        let state = coverage_state(check.state)?;
        if check.reserved != 0
            || (producer == AnnotationProducer::Native && state == AnnotationCheckState::Pending)
        {
            return Err(invalid_result());
        }
        let slot = &mut states[check.producer as usize - 1][check.dimension as usize - 1];
        if slot.replace(state).is_some() {
            return Err(invalid_result());
        }
        owned_checks.push(AnnotationCoverageCheck {
            producer,
            dimension,
            state,
        });
    }
    let mut owned_issues = reserve(issues.len())?;
    let mut seen = [[false; 8]; 3];
    for issue in issues {
        let producer = coverage_producer(issue.producer)?;
        let dimension = coverage_dimension(issue.dimension)?;
        let reason = coverage_reason(issue.reason)?;
        if states[issue.producer as usize - 1][issue.dimension as usize - 1]
            != Some(AnnotationCheckState::Unverified)
        {
            return Err(invalid_result());
        }
        let (scope, source) = coverage_issue_location(issue, marks, sources)?;
        seen[issue.producer as usize - 1][issue.dimension as usize - 1] = true;
        owned_issues.push(AnnotationCoverageIssue {
            producer,
            dimension,
            reason,
            scope,
            source,
        });
    }
    if states.iter().zip(seen).any(|(row, seen_row)| {
        row.iter().zip(seen_row).any(|(state, seen_issue)| {
            state.is_none() || (*state == Some(AnnotationCheckState::Unverified)) != seen_issue
        })
    }) {
        return Err(invalid_result());
    }
    Ok(AnnotationCoverage {
        checks: owned_checks,
        issues: owned_issues,
    })
}
