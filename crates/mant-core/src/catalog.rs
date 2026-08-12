//! Unifies registered Markdown and indexed manual pages for discovery clients.

use std::{error::Error, fmt, path::PathBuf};

use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use mant_ast::{
    CatalogDocumentKind, CatalogMatchRank, CatalogQuery, CatalogSchema, DocumentAddress,
    DocumentCatalog, DocumentSummary, MarkdownOrigin, SearchCase, SearchSyntax,
    catalog_literal_match_rank,
};

use mant_sources::{
    RegisteredDocument, RegisteredDocumentOrigin, SourceConfigError, list_registered_documents,
};

use crate::{ManualIndex, discover_manual_roots};

/// Source family used to resolve one available document.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AvailableDocumentKind {
    Markdown,
    Manual,
}

/// Precedence class and storage family for one available document.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AvailableDocumentOrigin {
    Documents,
    Source(String),
    ManualPath,
}

/// One document discoverable by name through the ordinary query boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailableDocument {
    pub name: String,
    pub kind: AvailableDocumentKind,
    pub section: Option<String>,
    pub path: PathBuf,
    pub origin: AvailableDocumentOrigin,
}

/// Invalid document-catalog filter or regular expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogError {
    EmptyPattern,
    PatternTooLong,
    InvalidLimit,
    ConflictingSelectors,
    InvalidPattern(String),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPattern => formatter.write_str("catalog pattern must not be empty"),
            Self::PatternTooLong => {
                formatter.write_str("catalog pattern exceeds the 4096-byte limit")
            }
            Self::InvalidLimit => formatter.write_str("catalog limit must be between 1 and 10000"),
            Self::ConflictingSelectors => {
                formatter.write_str("catalog source and section filters cannot be combined")
            }
            Self::InvalidPattern(message) => {
                write!(formatter, "invalid catalog pattern: {message}")
            }
        }
    }
}

impl Error for CatalogError {}

/// List every registered document candidate and locally indexed manual page.
///
/// # Errors
///
/// Returns an error when the platform data root or source configuration cannot
/// be read or validated.
pub fn list_available_documents() -> Result<Vec<AvailableDocument>, SourceConfigError> {
    let manuals = ManualIndex::from_roots(discover_manual_roots());
    Ok(list_available_documents_from(
        list_registered_documents()?,
        manuals.pages(),
    ))
}

/// Filter the unified local catalog using one shared CLI, TUI, and MCP policy.
///
/// # Errors
///
/// Returns a validation or regular-expression error without reading documents.
pub fn query_available_documents(
    documents: Vec<AvailableDocument>,
    query: &CatalogQuery,
) -> Result<DocumentCatalog, CatalogError> {
    validate_catalog_query(query)?;
    let compiled_pattern = query
        .pattern
        .as_deref()
        .map(|pattern| build_matcher(pattern, query.syntax, query.case))
        .transpose()?;
    let mut filtered = documents
        .into_iter()
        .filter(|document| {
            query.kind.is_none_or(|kind| match kind {
                CatalogDocumentKind::Markdown => document.kind == AvailableDocumentKind::Markdown,
                CatalogDocumentKind::Manual => document.kind == AvailableDocumentKind::Manual,
            }) && query.section.as_ref().is_none_or(|section| {
                document.section.as_ref().is_some_and(|value| value == section)
            }) && query.source.as_ref().is_none_or(|source| {
                matches!(&document.origin, AvailableDocumentOrigin::Source(value) if value == source)
            })
        })
        .filter_map(|document| {
            let matched = compiled_pattern
                .as_ref()
                .map_or(Ok(true), |matcher| matcher.is_match(document.name.as_bytes()));
            matched.ok().filter(|matched| *matched).map(|_| document)
        })
        .collect::<Vec<_>>();
    filtered.sort_by(|left, right| {
        match_rank(left.name.as_str(), query)
            .cmp(&match_rank(right.name.as_str(), query))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.section.cmp(&right.section))
            .then_with(|| left.origin.cmp(&right.origin))
    });

    let total = filtered.len();
    let offset = usize::try_from(query.offset)
        .unwrap_or(usize::MAX)
        .min(total);
    let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
    let end = offset.saturating_add(limit).min(total);
    let documents = filtered[offset..end]
        .iter()
        .map(document_summary)
        .collect::<Vec<_>>();
    Ok(DocumentCatalog {
        schema: CatalogSchema::V7,
        total: u32::try_from(total).unwrap_or(u32::MAX),
        returned: u32::try_from(documents.len()).unwrap_or(u32::MAX),
        offset: u32::try_from(offset).unwrap_or(u32::MAX),
        truncated: end < total,
        next_offset: (end < total).then(|| u32::try_from(end).unwrap_or(u32::MAX)),
        documents,
    })
}

/// Load and query the current local document catalog.
///
/// # Errors
///
/// Returns source configuration or catalog validation failures as text because
/// both are operational boundaries for every frontend.
pub fn discover_documents(query: &CatalogQuery) -> Result<DocumentCatalog, String> {
    let documents = list_available_documents().map_err(|error| error.to_string())?;
    query_available_documents(documents, query).map_err(|error| error.to_string())
}

fn validate_catalog_query(query: &CatalogQuery) -> Result<(), CatalogError> {
    if query.pattern.as_deref().is_some_and(str::is_empty) {
        return Err(CatalogError::EmptyPattern);
    }
    if query
        .pattern
        .as_ref()
        .is_some_and(|pattern| pattern.len() > 4096)
    {
        return Err(CatalogError::PatternTooLong);
    }
    if query.limit == 0 || query.limit > 10_000 {
        return Err(CatalogError::InvalidLimit);
    }
    if query.source.is_some() && query.section.is_some() {
        return Err(CatalogError::ConflictingSelectors);
    }
    if query.source.is_some() && query.kind == Some(CatalogDocumentKind::Manual)
        || query.section.is_some() && query.kind == Some(CatalogDocumentKind::Markdown)
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

fn match_rank(name: &str, query: &CatalogQuery) -> CatalogMatchRank {
    if query.syntax == SearchSyntax::Literal {
        catalog_literal_match_rank(name, query.pattern.as_deref(), query.case)
    } else {
        CatalogMatchRank::Unranked
    }
}

fn document_summary(document: &AvailableDocument) -> DocumentSummary {
    let address = match &document.origin {
        AvailableDocumentOrigin::Documents => DocumentAddress::Markdown {
            name: document.name.clone(),
            origin: MarkdownOrigin::Documents,
        },
        AvailableDocumentOrigin::Source(source) => DocumentAddress::Markdown {
            name: document.name.clone(),
            origin: MarkdownOrigin::Source {
                name: source.clone(),
            },
        },
        AvailableDocumentOrigin::ManualPath => DocumentAddress::Manual {
            name: document.name.clone(),
            section: document.section.clone().unwrap_or_default(),
        },
    };
    DocumentSummary {
        address,
        path: document.path.to_string_lossy().into_owned(),
    }
}

pub(crate) fn list_available_documents_from(
    registered: Vec<RegisteredDocument>,
    manuals: &[crate::ManualPage],
) -> Vec<AvailableDocument> {
    let mut documents = registered
        .into_iter()
        .map(|document| AvailableDocument {
            name: document.name,
            kind: AvailableDocumentKind::Markdown,
            section: None,
            path: document.path,
            origin: match document.origin {
                RegisteredDocumentOrigin::Documents => AvailableDocumentOrigin::Documents,
                RegisteredDocumentOrigin::Source(source) => AvailableDocumentOrigin::Source(source),
            },
        })
        .chain(manuals.iter().map(|page| AvailableDocument {
            name: page.name.clone(),
            kind: AvailableDocumentKind::Manual,
            section: Some(page.section.clone()),
            path: page.path.clone(),
            origin: AvailableDocumentOrigin::ManualPath,
        }))
        .collect::<Vec<_>>();
    documents.sort_by(|left, right| {
        (&left.name, left.kind, &left.section).cmp(&(&right.name, right.kind, &right.section))
    });
    documents
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mant_sources::{RegisteredDocument, RegisteredDocumentOrigin};

    use crate::ManualPage;

    use mant_ast::{CatalogDocumentKind, CatalogQuery, DocumentAddress, SearchCase, SearchSyntax};

    use super::{
        AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin,
        list_available_documents_from, query_available_documents,
    };

    #[test]
    fn merges_both_namespaces_without_hiding_manual_sections() {
        let documents = list_available_documents_from(
            vec![RegisteredDocument {
                name: "printf".to_owned(),
                path: PathBuf::from("/home/demo/.local/share/mant/documents/printf.md"),
                origin: RegisteredDocumentOrigin::Documents,
            }],
            &[
                ManualPage {
                    name: "printf".to_owned(),
                    section: "1".to_owned(),
                    path: PathBuf::from("/usr/share/man/man1/printf.1.gz"),
                    manual_root: PathBuf::from("/usr/share/man"),
                },
                ManualPage {
                    name: "printf".to_owned(),
                    section: "3".to_owned(),
                    path: PathBuf::from("/usr/share/man/man3/printf.3.gz"),
                    manual_root: PathBuf::from("/usr/share/man"),
                },
            ],
        );

        assert_eq!(documents.len(), 3);
        assert_eq!(documents[0].kind, AvailableDocumentKind::Markdown);
        assert_eq!(documents[0].origin, AvailableDocumentOrigin::Documents);
        assert_eq!(documents[1].section.as_deref(), Some("1"));
        assert_eq!(documents[2].section.as_deref(), Some("3"));
    }

    #[test]
    fn keeps_shadowed_markdown_candidates_in_fallback_order() {
        let documents = list_available_documents_from(
            vec![
                RegisteredDocument {
                    name: "tool".to_owned(),
                    path: PathBuf::from("/data/mant/documents/tool.md"),
                    origin: RegisteredDocumentOrigin::Documents,
                },
                RegisteredDocument {
                    name: "tool".to_owned(),
                    path: PathBuf::from("/data/mant/documents/sources/alpha/tool.md"),
                    origin: RegisteredDocumentOrigin::Source("alpha".to_owned()),
                },
            ],
            &[],
        );
        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0].origin, AvailableDocumentOrigin::Documents);
        assert_eq!(
            documents[1].origin,
            AvailableDocumentOrigin::Source("alpha".to_owned())
        );
    }

    #[test]
    fn catalog_search_ranks_exact_prefix_and_substring_matches() {
        let documents = ["process", "Start-Process", "process-tree"]
            .into_iter()
            .map(|name| AvailableDocument {
                name: name.to_owned(),
                kind: AvailableDocumentKind::Markdown,
                section: None,
                path: PathBuf::from(format!("/data/{name}.md")),
                origin: AvailableDocumentOrigin::Source("pwsh7".to_owned()),
            })
            .collect();
        let catalog = query_available_documents(
            documents,
            &CatalogQuery {
                pattern: Some("process".to_owned()),
                limit: 10,
                ..CatalogQuery::default()
            },
        )
        .expect("catalog");

        assert_eq!(catalog.total, 3);
        assert_eq!(catalog.documents[0].address.name(), "process");
        assert_eq!(catalog.documents[1].address.name(), "process-tree");
        assert_eq!(catalog.documents[2].address.name(), "Start-Process");
    }

    #[test]
    fn catalog_puts_an_exact_manual_before_every_prefix_and_substring() {
        let documents = ["woman", "manpath", "man", "man.conf"]
            .into_iter()
            .map(|name| AvailableDocument {
                name: name.to_owned(),
                kind: AvailableDocumentKind::Manual,
                section: Some("1".to_owned()),
                path: PathBuf::from(format!("/man/{name}.1")),
                origin: AvailableDocumentOrigin::ManualPath,
            })
            .collect();
        let catalog = query_available_documents(
            documents,
            &CatalogQuery {
                pattern: Some("man".to_owned()),
                limit: 10,
                ..CatalogQuery::default()
            },
        )
        .expect("catalog");
        let names = catalog
            .documents
            .iter()
            .map(|document| document.address.name())
            .collect::<Vec<_>>();

        assert_eq!(names, ["man", "man.conf", "manpath", "woman"]);
    }

    #[test]
    fn catalog_filters_keep_manual_sections_and_exact_addresses() {
        let documents = vec![
            AvailableDocument {
                name: "printf".to_owned(),
                kind: AvailableDocumentKind::Manual,
                section: Some("1".to_owned()),
                path: PathBuf::from("/man/printf.1"),
                origin: AvailableDocumentOrigin::ManualPath,
            },
            AvailableDocument {
                name: "printf".to_owned(),
                kind: AvailableDocumentKind::Manual,
                section: Some("3".to_owned()),
                path: PathBuf::from("/man/printf.3"),
                origin: AvailableDocumentOrigin::ManualPath,
            },
        ];
        let catalog = query_available_documents(
            documents,
            &CatalogQuery {
                pattern: Some("^PRINT".to_owned()),
                syntax: SearchSyntax::Regex,
                case: SearchCase::Insensitive,
                kind: Some(CatalogDocumentKind::Manual),
                section: Some("3".to_owned()),
                limit: 10,
                ..CatalogQuery::default()
            },
        )
        .expect("catalog");

        assert_eq!(catalog.documents.len(), 1);
        assert_eq!(
            catalog.documents[0].address,
            DocumentAddress::Manual {
                name: "printf".to_owned(),
                section: "3".to_owned(),
            }
        );
    }
}
