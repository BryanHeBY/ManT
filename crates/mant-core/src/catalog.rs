//! Unifies registered Markdown and indexed manual pages for discovery clients.

use std::path::PathBuf;

use crate::{RegisteredDocumentOrigin, list_registered_documents, system_manual_index};

/// Source family used to resolve one available document.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AvailableDocumentKind {
    Markdown,
    Manual,
}

/// Precedence class and storage family for one available document.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AvailableDocumentOrigin {
    User,
    System,
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

/// List every effective registered document and locally indexed manual page.
#[must_use]
pub fn list_available_documents() -> Vec<AvailableDocument> {
    list_available_documents_from(list_registered_documents(), system_manual_index().pages())
}

fn list_available_documents_from(
    registered: Vec<crate::RegisteredDocument>,
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
                RegisteredDocumentOrigin::User => AvailableDocumentOrigin::User,
                RegisteredDocumentOrigin::System => AvailableDocumentOrigin::System,
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
    documents.sort_unstable_by(|left, right| {
        (&left.name, left.kind, &left.section).cmp(&(&right.name, right.kind, &right.section))
    });
    documents
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{ManualPage, RegisteredDocument, RegisteredDocumentOrigin};

    use super::{AvailableDocumentKind, AvailableDocumentOrigin, list_available_documents_from};

    #[test]
    fn merges_both_namespaces_without_hiding_manual_sections() {
        let documents = list_available_documents_from(
            vec![RegisteredDocument {
                name: "printf".to_owned(),
                path: PathBuf::from("/home/demo/.local/share/mant/printf.md"),
                origin: RegisteredDocumentOrigin::User,
            }],
            &[
                ManualPage {
                    name: "printf".to_owned(),
                    section: "1".to_owned(),
                    path: PathBuf::from("/usr/share/man/man1/printf.1.gz"),
                },
                ManualPage {
                    name: "printf".to_owned(),
                    section: "3".to_owned(),
                    path: PathBuf::from("/usr/share/man/man3/printf.3.gz"),
                },
            ],
        );

        assert_eq!(documents.len(), 3);
        assert_eq!(documents[0].kind, AvailableDocumentKind::Markdown);
        assert_eq!(documents[0].origin, AvailableDocumentOrigin::User);
        assert_eq!(documents[1].section.as_deref(), Some("1"));
        assert_eq!(documents[2].section.as_deref(), Some("3"));
    }
}
