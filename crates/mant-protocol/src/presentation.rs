//! Deterministic, transport-neutral presentations of protocol projections.

use std::{borrow::Cow, collections::BTreeMap, fmt::Write as _};

use mant_ir::{DocumentAddress, MarkdownOrigin};

use crate::DocumentCatalog;

/// Replace control characters in dynamic text before terminal presentation.
///
/// Logical document identities and diagnostics can originate in local file
/// names or parser input. JSON keeps those values as data, while text-oriented
/// frontends must never let them inject terminal control sequences or extra
/// display lines.
#[must_use]
pub fn sanitize_terminal_text(value: &str) -> Cow<'_, str> {
    if !value.chars().any(char::is_control) {
        return Cow::Borrowed(value);
    }
    Cow::Owned(
        value
            .chars()
            .map(|character| {
                if character.is_control() {
                    '\u{fffd}'
                } else {
                    character
                }
            })
            .collect(),
    )
}

/// Explain why an empty catalog query selected no indexable scope.
///
/// A covered scope with no name matches intentionally returns `None`; callers
/// can retain their ordinary grep-like empty-result behavior.
#[must_use]
pub fn render_catalog_coverage_text(catalog: &DocumentCatalog) -> Option<String> {
    if catalog.total != 0 || catalog.coverage.scope_total != 0 {
        return None;
    }
    if let Some(section) = &catalog.query.manual_section {
        let mut message = format!(
            "no manuals indexed for section '{}'",
            sanitize_terminal_text(section)
        );
        if !catalog.coverage.manual_sections.is_empty() {
            message.push_str("\nindexed manual sections: ");
            message.push_str(&catalog.coverage.manual_sections.join(", "));
        }
        return Some(message);
    }
    if let Some(source) = &catalog.query.source {
        let mut message = format!(
            "source '{}' has no indexed Markdown documents",
            sanitize_terminal_text(source)
        );
        if !catalog.coverage.markdown_sources.is_empty() {
            message.push_str("\nindexed Markdown sources: ");
            message.push_str(&catalog.coverage.markdown_sources.join(", "));
        }
        return Some(message);
    }
    match catalog.query.kind {
        Some(crate::CatalogDocumentKind::Manual) => Some("no manuals indexed".to_owned()),
        Some(crate::CatalogDocumentKind::Markdown) => {
            Some("no Markdown documents indexed".to_owned())
        }
        None => Some("no documents indexed".to_owned()),
    }
}

/// Render a catalog page as stable, unstyled text.
///
/// Flat output is one `<catalog-path>\t<kind>` row per document. Grouped
/// output uses catalog namespaces as headings and indents their document
/// names. Neither form contains terminal escape sequences.
#[must_use]
pub fn render_catalog_text(catalog: &DocumentCatalog, grouped: bool) -> String {
    if !grouped {
        let mut output = String::new();
        for document in &catalog.documents {
            let (_, kind) = catalog_category(&document.address);
            writeln!(
                output,
                "{}\t{kind}",
                sanitize_terminal_text(&document.catalog_path())
            )
            .expect("writing to String cannot fail");
        }
        return output;
    }

    let mut categories = BTreeMap::<String, Vec<&str>>::new();
    for document in &catalog.documents {
        let (category, _) = catalog_category(&document.address);
        categories
            .entry(category)
            .or_default()
            .push(match &document.address {
                DocumentAddress::Markdown { path, .. } => path,
                DocumentAddress::Manual { name, .. } => name,
            });
    }
    let mut output = String::new();
    for (index, (category, names)) in categories.into_iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&sanitize_terminal_text(&category));
        output.push('\n');
        for name in names {
            output.push_str("  ");
            output.push_str(&sanitize_terminal_text(name));
            output.push('\n');
        }
    }
    output
}

fn catalog_category(address: &DocumentAddress) -> (String, &'static str) {
    match address {
        DocumentAddress::Markdown {
            origin: MarkdownOrigin::Documents,
            ..
        } => ("documents".to_owned(), "markdown"),
        DocumentAddress::Markdown {
            origin: MarkdownOrigin::Source { name },
            ..
        } => (format!("sources/{name}"), "markdown"),
        DocumentAddress::Manual { manual_section, .. } => {
            (format!("manual/{manual_section}"), "manual")
        }
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::{DocumentAddress, MarkdownOrigin};

    use crate::{
        CatalogCoverage, CatalogDocumentKind, CatalogQuery, CatalogSchema, DocumentCatalog,
        DocumentSummary,
    };

    use super::{render_catalog_coverage_text, render_catalog_text, sanitize_terminal_text};

    #[test]
    fn masks_terminal_controls_without_changing_unicode_text() {
        assert_eq!(sanitize_terminal_text("safe → text"), "safe → text");
        assert_eq!(
            sanitize_terminal_text("bad\u{1b}[31m\nname"),
            "bad�[31m�name"
        );
    }

    fn catalog() -> DocumentCatalog {
        let addresses = [
            DocumentAddress::Markdown {
                path: "mant".to_owned(),
                origin: MarkdownOrigin::Documents,
            },
            DocumentAddress::Manual {
                name: "git".to_owned(),
                manual_section: "1".to_owned(),
            },
        ];
        DocumentCatalog {
            schema: CatalogSchema::V0Dot11,
            query: crate::CatalogQuery::default(),
            coverage: crate::CatalogCoverage::default(),
            total: 2,
            returned: 2,
            offset: 0,
            truncated: false,
            next_offset: None,
            documents: addresses
                .into_iter()
                .map(|address| DocumentSummary { address })
                .collect(),
        }
    }

    #[test]
    fn flat_catalog_text_is_compact_and_machine_copyable() {
        assert_eq!(
            render_catalog_text(&catalog(), false),
            "documents/mant\tmarkdown\nmanual/1/git\tmanual\n"
        );
    }

    #[test]
    fn grouped_catalog_text_preserves_catalog_namespaces() {
        assert_eq!(
            render_catalog_text(&catalog(), true),
            "documents\n  mant\n\nmanual/1\n  git\n"
        );
    }

    #[test]
    fn catalog_text_masks_controls_from_logical_addresses() {
        let mut catalog = catalog();
        catalog.documents[0].address = DocumentAddress::Manual {
            name: "tool\u{1b}[2J\nnext".to_owned(),
            manual_section: "1".to_owned(),
        };

        let rendered = render_catalog_text(&catalog, false);
        assert_eq!(
            rendered,
            "manual/1/tool�[2J�next\tmanual\nmanual/1/git\tmanual\n"
        );
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn empty_catalog_explains_only_an_unindexed_scope() {
        let unindexed = DocumentCatalog {
            query: CatalogQuery {
                kind: Some(CatalogDocumentKind::Manual),
                manual_section: Some("42".to_owned()),
                ..CatalogQuery::default()
            },
            coverage: CatalogCoverage {
                scope_total: 0,
                manual_sections: vec!["1".to_owned(), "2".to_owned(), "2const".to_owned()],
                ..CatalogCoverage::default()
            },
            ..DocumentCatalog::default()
        };
        assert_eq!(
            render_catalog_coverage_text(&unindexed).as_deref(),
            Some("no manuals indexed for section '42'\nindexed manual sections: 1, 2, 2const")
        );

        let covered = DocumentCatalog {
            coverage: CatalogCoverage {
                scope_total: 12,
                ..CatalogCoverage::default()
            },
            ..unindexed
        };
        assert_eq!(render_catalog_coverage_text(&covered), None);
    }
}
