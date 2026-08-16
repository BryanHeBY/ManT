//! Unifies registered Markdown and indexed manual pages for discovery clients.

use std::{error::Error, fmt, path::PathBuf};

use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use mant_protocol::{
    CatalogDocumentKind, CatalogMatchRank, CatalogQuery, CatalogSchema, DocumentAddress,
    DocumentCatalog, DocumentSummary, MarkdownOrigin, SearchCase, SearchSyntax,
};

use mant_sources::{
    BUILTIN_CONTENT_PRIORITY, RegisteredDocument, RegisteredDocumentOrigin, SourceConfigError,
    list_registered_documents,
};

use crate::{ManualIndex, discover_manual_roots};

/// Source family used to resolve one available document.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AvailableDocumentKind {
    /// Registered Markdown document.
    Markdown,
    /// Indexed native manual page.
    Manual,
}

/// Precedence class and storage family for one available document.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AvailableDocumentOrigin {
    /// User-authored primary documents tree.
    Documents,
    /// One configured source cache, named by its configuration key.
    Source(String),
    /// A directory discovered through the native manual search path.
    ManualPath,
}

/// One document discoverable by name through the ordinary query boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AvailableDocument {
    /// Short lookup name.
    pub name: String,
    /// Extension-free path relative to this document's origin.
    pub logical_path: String,
    /// Broad source format family.
    pub kind: AvailableDocumentKind,
    /// Native manual category, present only for manual pages.
    pub manual_section: Option<String>,
    /// Physical local source path.
    pub path: PathBuf,
    /// Storage namespace and precedence class.
    pub origin: AvailableDocumentOrigin,
    /// Configured priority relative to native manuals, or `None` otherwise.
    pub source_priority: Option<i32>,
}

/// Invalid document-catalog filter or regular expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogError {
    /// An explicit pattern contained no text.
    EmptyPattern,
    /// A pattern exceeded the bounded request size.
    PatternTooLong,
    /// Pagination limit was zero or exceeded the protocol maximum.
    InvalidLimit,
    /// Source-family filters cannot describe any valid document.
    ConflictingSelectors,
    /// A regular expression could not be compiled.
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
                formatter.write_str("catalog source and manual-section filters cannot be combined")
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
    documents: &[AvailableDocument],
    query: &CatalogQuery,
) -> Result<DocumentCatalog, CatalogError> {
    validate_catalog_query(query)?;
    let compiled_pattern = query
        .pattern
        .as_deref()
        .map(|pattern| build_matcher(pattern, query.syntax, query.case))
        .transpose()?;
    let mut filtered = documents
        .iter()
        .filter(|document| {
            query.kind.is_none_or(|kind| match kind {
                CatalogDocumentKind::Markdown => document.kind == AvailableDocumentKind::Markdown,
                CatalogDocumentKind::Manual => document.kind == AvailableDocumentKind::Manual,
            }) && query.manual_section.as_ref().is_none_or(|section| {
                document.manual_section.as_ref().is_some_and(|value| value == section)
            }) && query.source.as_ref().is_none_or(|source| {
                matches!(&document.origin, AvailableDocumentOrigin::Source(value) if value == source)
            })
        })
        .filter_map(|document| {
            let match_catalog_path = query
                .pattern
                .as_deref()
                .is_some_and(|pattern| pattern.contains('/'));
            let matched = compiled_pattern.as_ref().map_or(Ok(true), |matcher| {
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
        match_rank(left, query)
            .cmp(&match_rank(right, query))
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
    let documents = filtered[offset..end]
        .iter()
        .copied()
        .map(document_summary)
        .collect::<Vec<_>>();
    Ok(DocumentCatalog {
        schema: CatalogSchema::V0Dot8,
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
    query_available_documents(&documents, query).map_err(|error| error.to_string())
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

fn match_rank(document: &AvailableDocument, query: &CatalogQuery) -> CatalogMatchRank {
    if query.syntax != SearchSyntax::Literal {
        return CatalogMatchRank::Unranked;
    }
    let Some(pattern) = query.pattern.as_deref() else {
        return CatalogMatchRank::Unranked;
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
        mant_protocol::catalog_literal_match_rank(candidate, Some(pattern), query.case)
    })
    .min()
    .unwrap_or(CatalogMatchRank::Unranked)
}

fn document_summary(document: &AvailableDocument) -> DocumentSummary {
    let address = match &document.origin {
        AvailableDocumentOrigin::Documents => DocumentAddress::Markdown {
            path: document.logical_path.clone(),
            origin: MarkdownOrigin::Documents,
        },
        AvailableDocumentOrigin::Source(source) => DocumentAddress::Markdown {
            path: document.logical_path.clone(),
            origin: MarkdownOrigin::Source {
                name: source.clone(),
            },
        },
        AvailableDocumentOrigin::ManualPath => DocumentAddress::Manual {
            name: document.name.clone(),
            manual_section: document.manual_section.clone().unwrap_or_default(),
        },
    };
    DocumentSummary {
        catalog_path: address.catalog_path(),
        address,
    }
}

fn available_catalog_path(document: &AvailableDocument) -> String {
    match &document.origin {
        AvailableDocumentOrigin::Documents => format!("documents/{}", document.logical_path),
        AvailableDocumentOrigin::Source(source) => {
            format!("sources/{source}/{}", document.logical_path)
        }
        AvailableDocumentOrigin::ManualPath => format!(
            "manual/{}/{}",
            document.manual_section.as_deref().unwrap_or_default(),
            document.name
        ),
    }
}

fn compare_precedence(left: &AvailableDocument, right: &AvailableDocument) -> std::cmp::Ordering {
    fn class(document: &AvailableDocument) -> u8 {
        match (&document.origin, document.source_priority) {
            (AvailableDocumentOrigin::Documents, _) => 0,
            (AvailableDocumentOrigin::Source(_), Some(priority))
                if priority > BUILTIN_CONTENT_PRIORITY =>
            {
                1
            }
            (AvailableDocumentOrigin::ManualPath, _) => 2,
            (AvailableDocumentOrigin::Source(_), _) => 3,
        }
    }

    class(left)
        .cmp(&class(right))
        .then_with(|| match (&left.origin, &right.origin) {
            (AvailableDocumentOrigin::Source(_), AvailableDocumentOrigin::Source(_)) => right
                .source_priority
                .unwrap_or_default()
                .cmp(&left.source_priority.unwrap_or_default()),
            _ => std::cmp::Ordering::Equal,
        })
}

pub(crate) fn list_available_documents_from(
    registered: Vec<RegisteredDocument>,
    manuals: &[crate::ManualPage],
) -> Vec<AvailableDocument> {
    let mut documents = registered
        .into_iter()
        .map(|document| AvailableDocument {
            name: document
                .logical_path
                .rsplit('/')
                .next()
                .unwrap_or(&document.logical_path)
                .to_owned(),
            logical_path: document.logical_path,
            kind: AvailableDocumentKind::Markdown,
            manual_section: None,
            path: document.path,
            source_priority: document.source_priority,
            origin: match document.origin {
                RegisteredDocumentOrigin::Documents => AvailableDocumentOrigin::Documents,
                RegisteredDocumentOrigin::Source(source) => AvailableDocumentOrigin::Source(source),
            },
        })
        .chain(manuals.iter().map(|page| AvailableDocument {
            name: page.name.clone(),
            logical_path: page.name.clone(),
            kind: AvailableDocumentKind::Manual,
            manual_section: Some(page.section.clone()),
            path: page.path.clone(),
            origin: AvailableDocumentOrigin::ManualPath,
            source_priority: None,
        }))
        .collect::<Vec<_>>();
    documents.sort_by(|left, right| {
        left.logical_path
            .cmp(&right.logical_path)
            .then_with(|| compare_precedence(left, right))
            .then_with(|| left.manual_section.cmp(&right.manual_section))
            .then_with(|| left.origin.cmp(&right.origin))
    });
    documents
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mant_sources::{RegisteredDocument, RegisteredDocumentOrigin};

    use crate::ManualPage;

    use mant_protocol::{
        CatalogDocumentKind, CatalogQuery, DocumentAddress, SearchCase, SearchSyntax,
    };

    use super::{
        AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin,
        list_available_documents_from, query_available_documents,
    };

    #[test]
    fn merges_both_namespaces_without_hiding_manual_sections() {
        let documents = list_available_documents_from(
            vec![RegisteredDocument {
                logical_path: "printf".to_owned(),
                path: PathBuf::from("/home/demo/.local/share/mant/documents/printf.md"),
                origin: RegisteredDocumentOrigin::Documents,
                source_priority: None,
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
        assert_eq!(documents[1].manual_section.as_deref(), Some("1"));
        assert_eq!(documents[2].manual_section.as_deref(), Some("3"));
    }

    #[test]
    fn keeps_shadowed_markdown_candidates_in_fallback_order() {
        let documents = list_available_documents_from(
            vec![
                RegisteredDocument {
                    logical_path: "tool".to_owned(),
                    path: PathBuf::from("/data/mant/documents/tool.md"),
                    origin: RegisteredDocumentOrigin::Documents,
                    source_priority: None,
                },
                RegisteredDocument {
                    logical_path: "tool".to_owned(),
                    path: PathBuf::from("/data/mant/sources/alpha/tool.md"),
                    origin: RegisteredDocumentOrigin::Source("alpha".to_owned()),
                    source_priority: Some(1),
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
    fn catalog_orders_sources_around_the_native_manual_zero_baseline() {
        let documents = list_available_documents_from(
            vec![
                RegisteredDocument {
                    logical_path: "tool".to_owned(),
                    path: PathBuf::from("/sources/low/tool.md"),
                    origin: RegisteredDocumentOrigin::Source("low".to_owned()),
                    source_priority: Some(-1),
                },
                RegisteredDocument {
                    logical_path: "tool".to_owned(),
                    path: PathBuf::from("/sources/high/tool.md"),
                    origin: RegisteredDocumentOrigin::Source("high".to_owned()),
                    source_priority: Some(1),
                },
                RegisteredDocument {
                    logical_path: "tool".to_owned(),
                    path: PathBuf::from("/sources/tie/tool.md"),
                    origin: RegisteredDocumentOrigin::Source("tie".to_owned()),
                    source_priority: Some(0),
                },
            ],
            &[ManualPage {
                name: "tool".to_owned(),
                section: "1".to_owned(),
                path: PathBuf::from("/man/tool.1"),
                manual_root: PathBuf::from("/man"),
            }],
        );

        assert_eq!(
            documents
                .iter()
                .map(|document| match &document.origin {
                    AvailableDocumentOrigin::Source(name) => format!("source:{name}"),
                    AvailableDocumentOrigin::ManualPath => "manual".to_owned(),
                    AvailableDocumentOrigin::Documents => "documents".to_owned(),
                })
                .collect::<Vec<_>>(),
            ["source:high", "manual", "source:tie", "source:low"]
        );
    }

    #[test]
    fn catalog_search_ranks_exact_prefix_and_substring_matches() {
        let documents = ["process", "Start-Process", "process-tree"]
            .into_iter()
            .map(|name| AvailableDocument {
                name: name.to_owned(),
                logical_path: name.to_owned(),
                kind: AvailableDocumentKind::Markdown,
                manual_section: None,
                path: PathBuf::from(format!("/data/{name}.md")),
                origin: AvailableDocumentOrigin::Source("pwsh7".to_owned()),
                source_priority: Some(1),
            })
            .collect::<Vec<_>>();
        let catalog = query_available_documents(
            &documents,
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
        let documents = ["woman", "manpath", "man", "man.conf", "printf"]
            .into_iter()
            .map(|name| AvailableDocument {
                name: name.to_owned(),
                logical_path: name.to_owned(),
                kind: AvailableDocumentKind::Manual,
                manual_section: Some("1".to_owned()),
                path: PathBuf::from(format!("/man/{name}.1")),
                origin: AvailableDocumentOrigin::ManualPath,
                source_priority: None,
            })
            .collect::<Vec<_>>();
        let catalog = query_available_documents(
            &documents,
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
    fn catalog_ranks_hierarchical_exact_suffix_prefix_and_substring_matches() {
        let documents = ["tool", "languages/en/tool", "toolbox", "guides/mytool"]
            .into_iter()
            .map(|logical_path| AvailableDocument {
                name: logical_path.rsplit('/').next().expect("leaf").to_owned(),
                logical_path: logical_path.to_owned(),
                kind: AvailableDocumentKind::Markdown,
                manual_section: None,
                path: PathBuf::from(format!("/documents/{logical_path}.md")),
                origin: AvailableDocumentOrigin::Documents,
                source_priority: None,
            })
            .collect::<Vec<_>>();
        let catalog = query_available_documents(
            &documents,
            &CatalogQuery {
                pattern: Some("tool".to_owned()),
                limit: 10,
                ..CatalogQuery::default()
            },
        )
        .expect("hierarchical catalog");
        assert_eq!(
            catalog
                .documents
                .iter()
                .map(|document| document.catalog_path.as_str())
                .collect::<Vec<_>>(),
            [
                "documents/languages/en/tool",
                "documents/tool",
                "documents/toolbox",
                "documents/guides/mytool",
            ]
        );

        let exact = AvailableDocument {
            name: "tool".to_owned(),
            logical_path: "en/tool".to_owned(),
            kind: AvailableDocumentKind::Markdown,
            manual_section: None,
            path: PathBuf::from("/documents/en/tool.md"),
            origin: AvailableDocumentOrigin::Documents,
            source_priority: None,
        };
        let catalog = query_available_documents(
            &[exact, documents[1].clone()],
            &CatalogQuery {
                pattern: Some("en/tool".to_owned()),
                limit: 10,
                ..CatalogQuery::default()
            },
        )
        .expect("component suffix catalog");
        assert_eq!(
            catalog
                .documents
                .iter()
                .map(|document| document.catalog_path.as_str())
                .collect::<Vec<_>>(),
            ["documents/en/tool", "documents/languages/en/tool"]
        );
    }

    #[test]
    fn catalog_filters_keep_manual_sections_and_exact_addresses() {
        let documents = vec![
            AvailableDocument {
                name: "printf".to_owned(),
                logical_path: "printf".to_owned(),
                kind: AvailableDocumentKind::Manual,
                manual_section: Some("1".to_owned()),
                path: PathBuf::from("/man/printf.1"),
                origin: AvailableDocumentOrigin::ManualPath,
                source_priority: None,
            },
            AvailableDocument {
                name: "printf".to_owned(),
                logical_path: "printf".to_owned(),
                kind: AvailableDocumentKind::Manual,
                manual_section: Some("3".to_owned()),
                path: PathBuf::from("/man/printf.3"),
                origin: AvailableDocumentOrigin::ManualPath,
                source_priority: None,
            },
        ];
        let catalog = query_available_documents(
            &documents,
            &CatalogQuery {
                pattern: Some("^PRINT".to_owned()),
                syntax: SearchSyntax::Regex,
                case: SearchCase::Insensitive,
                kind: Some(CatalogDocumentKind::Manual),
                manual_section: Some("3".to_owned()),
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
                manual_section: "3".to_owned(),
            }
        );
    }
}
