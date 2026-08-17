//! Deterministic, transport-neutral presentations of protocol projections.

use std::{collections::BTreeMap, fmt::Write as _};

use mant_ir::{DocumentAddress, MarkdownOrigin};

use crate::DocumentCatalog;

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
            writeln!(output, "{}\t{kind}", document.catalog_path)
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
        output.push_str(&category);
        output.push('\n');
        for name in names {
            output.push_str("  ");
            output.push_str(name);
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

    use crate::{CatalogSchema, DocumentCatalog, DocumentSummary};

    use super::render_catalog_text;

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
            schema: CatalogSchema::V0Dot8,
            total: 2,
            returned: 2,
            offset: 0,
            truncated: false,
            next_offset: None,
            documents: addresses
                .into_iter()
                .map(|address| DocumentSummary {
                    catalog_path: address.catalog_path(),
                    address,
                })
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
}
