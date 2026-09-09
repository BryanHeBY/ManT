//! Validates, matches, ranks, and pages an immutable catalog without source IO.

use super::inventory::{available_catalog_path, compare_precedence, document_summary};
use super::{AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin, CatalogError};
use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use mant_protocol::{
    CatalogCoverage, CatalogDocumentKind, CatalogMatchScore, CatalogQuery, CatalogSchema,
    DocumentCatalog, MAX_CATALOG_PATTERN_CHARS, SearchCase, SearchSyntax,
};
use std::collections::BTreeSet;

/// Filter the unified local catalog using one shared CLI, TUI, and MCP policy.
///
/// # Errors
///
/// Returns a validation or regular-expression error without reading documents.
pub fn query_available_documents(
    documents: &[AvailableDocument],
    query: &CatalogQuery,
) -> Result<DocumentCatalog, CatalogError> {
    Ok(PreparedCatalogQuery::new(query)?.apply(documents))
}

/// Validated filters and one compiled matcher, prepared without source discovery.
///
/// Prepare this borrowed query before creating a system loader when invalid
/// requests must perform no configuration reads. It can be applied repeatedly
/// to the same explicit catalog or passed to a loader without compiling the
/// matcher again. This type neither owns nor refreshes a document snapshot.
pub struct PreparedCatalogQuery<'query> {
    query: &'query CatalogQuery,
    compiled_pattern: Option<grep_regex::RegexMatcher>,
}

impl<'query> PreparedCatalogQuery<'query> {
    /// Validate bounds and compile the optional literal or regular expression.
    ///
    /// # Errors
    /// Returns invalid filters, bounds or pattern syntax without source IO.
    pub fn new(query: &'query CatalogQuery) -> Result<Self, CatalogError> {
        validate_catalog_query(query)?;
        let compiled_pattern = query
            .pattern
            .as_deref()
            .map(|pattern| build_matcher(pattern, query.syntax, query.case))
            .transpose()?;
        Ok(Self {
            query,
            compiled_pattern,
        })
    }

    /// Filter, rank and page an already materialized catalog without IO.
    #[must_use]
    pub fn apply(&self, documents: &[AvailableDocument]) -> DocumentCatalog {
        let query = self.query;
        let in_scope = |document: &&AvailableDocument| catalog_scope_matches(document, query);
        let scope_total = documents.iter().filter(in_scope).count();
        let mut filtered = documents
            .iter()
            .filter(in_scope)
            .filter_map(|document| {
                let match_catalog_path = query
                    .pattern
                    .as_deref()
                    .is_some_and(|pattern| pattern.contains('/'));
                let matched = self.compiled_pattern.as_ref().map_or(Ok(true), |matcher| {
                    matcher
                        .is_match(document.name.as_bytes())
                        .and_then(|matched| {
                            if matched {
                                Ok(true)
                            } else {
                                matcher.is_match(document.logical_path.as_bytes())
                            }
                        })
                        .and_then(|matched| {
                            if matched {
                                Ok(true)
                            } else if !match_catalog_path {
                                Ok(false)
                            } else {
                                matcher.is_match(available_catalog_path(document).as_bytes())
                            }
                        })
                });
                matched.ok().filter(|matched| *matched).map(|_| document)
            })
            .collect::<Vec<_>>();
        filtered.sort_by(|left, right| {
            match_score(left, query)
                .cmp(&match_score(right, query))
                .then_with(|| {
                    left.logical_path
                        .to_lowercase()
                        .cmp(&right.logical_path.to_lowercase())
                })
                .then_with(|| left.logical_path.cmp(&right.logical_path))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| compare_precedence(left, right))
                .then_with(|| left.manual_section.cmp(&right.manual_section))
                .then_with(|| left.origin.cmp(&right.origin))
        });

        let total = filtered.len();
        let offset = usize::try_from(query.offset)
            .unwrap_or(usize::MAX)
            .min(total);
        let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
        let end = offset.saturating_add(limit).min(total);
        let coverage = catalog_coverage(documents, scope_total);
        let documents = filtered[offset..end]
            .iter()
            .copied()
            .map(document_summary)
            .collect::<Vec<_>>();
        DocumentCatalog {
            schema: CatalogSchema::V0Dot11,
            query: query.clone(),
            coverage,
            total: u32::try_from(total).unwrap_or(u32::MAX),
            returned: u32::try_from(documents.len()).unwrap_or(u32::MAX),
            offset: u32::try_from(offset).unwrap_or(u32::MAX),
            truncated: end < total,
            next_offset: (end < total).then(|| u32::try_from(end).unwrap_or(u32::MAX)),
            documents,
        }
    }
}

fn catalog_scope_matches(document: &AvailableDocument, query: &CatalogQuery) -> bool {
    query.kind.is_none_or(|kind| match kind {
        CatalogDocumentKind::Markdown => document.kind == AvailableDocumentKind::Markdown,
        CatalogDocumentKind::Manual => document.kind == AvailableDocumentKind::Manual,
    }) && query.manual_section.as_ref().is_none_or(|section| {
        document
            .manual_section
            .as_ref()
            .is_some_and(|value| value == section)
    }) && query.source.as_ref().is_none_or(|source| {
        matches!(&document.origin, AvailableDocumentOrigin::Source(value) if value == source)
    })
}

fn catalog_coverage(documents: &[AvailableDocument], scope_total: usize) -> CatalogCoverage {
    let mut manual_sections = BTreeSet::new();
    let mut markdown_sources = BTreeSet::new();
    let mut personal_documents = false;
    for document in documents {
        match &document.origin {
            AvailableDocumentOrigin::Documents => personal_documents = true,
            AvailableDocumentOrigin::Source(source) => {
                markdown_sources.insert(source.clone());
            }
            AvailableDocumentOrigin::ManualPath => {
                if let Some(section) = &document.manual_section {
                    manual_sections.insert(section.clone());
                }
            }
        }
    }
    CatalogCoverage {
        scope_total: u32::try_from(scope_total).unwrap_or(u32::MAX),
        manual_sections: manual_sections.into_iter().collect(),
        markdown_sources: markdown_sources.into_iter().collect(),
        personal_documents,
    }
}

fn validate_catalog_query(query: &CatalogQuery) -> Result<(), CatalogError> {
    if query.pattern.as_deref().is_some_and(str::is_empty) {
        return Err(CatalogError::EmptyPattern);
    }
    if query
        .pattern
        .as_ref()
        .is_some_and(|pattern| pattern.chars().count() > MAX_CATALOG_PATTERN_CHARS)
    {
        return Err(CatalogError::PatternTooLong);
    }
    if query.limit == 0 || query.limit > 10_000 {
        return Err(CatalogError::InvalidLimit);
    }
    if query.source.is_some() && query.manual_section.is_some() {
        return Err(CatalogError::ConflictingSelectors);
    }
    if query.source.is_some() && query.kind == Some(CatalogDocumentKind::Manual)
        || query.manual_section.is_some() && query.kind == Some(CatalogDocumentKind::Markdown)
    {
        return Err(CatalogError::ConflictingSelectors);
    }
    Ok(())
}

fn build_matcher(
    pattern: &str,
    syntax: SearchSyntax,
    case: SearchCase,
) -> Result<grep_regex::RegexMatcher, CatalogError> {
    let mut builder = RegexMatcherBuilder::new();
    builder.fixed_strings(syntax == SearchSyntax::Literal);
    match case {
        SearchCase::Insensitive => {
            builder.case_insensitive(true);
        }
        SearchCase::Sensitive => {
            builder.case_insensitive(false);
        }
        SearchCase::Smart => {
            builder.case_smart(true);
        }
    }
    builder
        .build(pattern)
        .map_err(|error| CatalogError::InvalidPattern(error.to_string()))
}

fn match_score(document: &AvailableDocument, query: &CatalogQuery) -> CatalogMatchScore {
    if query.syntax != SearchSyntax::Literal {
        return mant_protocol::catalog_literal_match_score("", None, query.case);
    }
    let Some(pattern) = query.pattern.as_deref() else {
        return mant_protocol::catalog_literal_match_score("", None, query.case);
    };
    let catalog_path = available_catalog_path(document);
    [
        Some(document.name.as_str()),
        Some(document.logical_path.as_str()),
        pattern.contains('/').then_some(catalog_path.as_str()),
    ]
    .into_iter()
    .flatten()
    .map(|candidate| {
        mant_protocol::catalog_literal_match_score(candidate, Some(pattern), query.case)
    })
    .min()
    .unwrap_or_else(|| mant_protocol::catalog_literal_match_score("", None, query.case))
}
