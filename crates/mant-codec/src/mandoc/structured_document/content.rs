use libmandoc_rs::structured::{ContentAtomKind, ContentRootKey, NativeLinkTarget, ProvenanceKey};
use mant_ir::{
    Diagnostic, DiagnosticImpact, DiagnosticLevel, Inline, LinkTarget, NodeId, Provenance,
    SourceSpan,
};

use super::{NativeProjectionError, NativeProseProjection};

pub(super) fn root_inlines(
    projection: &NativeProseProjection,
    root: ContentRootKey,
) -> Result<Vec<Inline>, NativeProjectionError> {
    let document = projection.document();
    let projected = projection
        .root(root)
        .ok_or(NativeProjectionError::InvalidRelation(
            "content root has no projected leaves",
        ))?;
    let mut inlines = Vec::new();
    for leaf in &projected.leaves {
        let atom =
            document
                .content_atom(leaf.atom())
                .ok_or(NativeProjectionError::InvalidRelation(
                    "projected leaf references an unknown atom",
                ))?;
        let leaf = match atom.kind() {
            ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
                if atom.style().is_literal() {
                    Inline::Code {
                        value: text.clone(),
                    }
                } else {
                    Inline::Text {
                        value: text.clone(),
                    }
                }
            }
            ContentAtomKind::BreakOpportunity => continue,
            ContentAtomKind::HardBreak => Inline::LineBreak,
        };
        let mut leaf = if atom.style().is_italic() {
            Inline::Emphasis {
                children: vec![leaf],
            }
        } else {
            leaf
        };
        if atom.style().is_bold() || atom.style().is_underline() {
            leaf = Inline::Strong {
                children: vec![leaf],
            };
        }
        if let Some(link) = atom.link() {
            let link = document
                .link(link)
                .ok_or(NativeProjectionError::InvalidRelation(
                    "atom references an unknown link",
                ))?;
            leaf = Inline::Link {
                target: lower_link(link.target()),
                title: link.title().map(ToOwned::to_owned),
                children: vec![leaf],
            };
        }
        inlines.push(leaf);
    }
    Ok(inlines)
}

fn lower_link(target: &NativeLinkTarget) -> LinkTarget {
    match target {
        NativeLinkTarget::External(uri) => LinkTarget::External { uri: uri.clone() },
        NativeLinkTarget::Email(address) => LinkTarget::Email {
            address: address.clone(),
        },
        NativeLinkTarget::Document(name) => LinkTarget::Document {
            name: name.clone(),
            fragment: None,
        },
        NativeLinkTarget::Manual { name, section } => LinkTarget::Manual {
            name: name.clone(),
            manual_section: Some(section.clone()),
        },
        NativeLinkTarget::Section(id) => LinkTarget::Section {
            id: NodeId::new(id.clone()),
        },
    }
}

pub(super) fn source_for(
    projection: &NativeProseProjection,
    key: ProvenanceKey,
) -> Option<SourceSpan> {
    match projection.provenances().get(key.get() as usize - 1)? {
        Provenance::Authored { span } => Some(*span),
        Provenance::Generated { .. } | Provenance::Unknown => None,
    }
}

pub(super) fn lower_diagnostics(projection: &NativeProseProjection) -> Vec<Diagnostic> {
    projection
        .document()
        .diagnostics()
        .iter()
        .map(|diagnostic| Diagnostic {
            level: match diagnostic.level() {
                libmandoc_rs::structured::StructuredDiagnosticLevel::Style => {
                    DiagnosticLevel::Style
                }
                libmandoc_rs::structured::StructuredDiagnosticLevel::Warning => {
                    DiagnosticLevel::Warning
                }
                libmandoc_rs::structured::StructuredDiagnosticLevel::Error => {
                    DiagnosticLevel::Error
                }
                libmandoc_rs::structured::StructuredDiagnosticLevel::Unsupported => {
                    DiagnosticLevel::Unsupported
                }
            },
            impact: DiagnosticImpact::None,
            code: Some(diagnostic.code().as_str().to_owned()),
            message: diagnostic.message().to_owned(),
            source: diagnostic
                .span()
                .and_then(|span| projection.spans().get(span.get() as usize - 1).copied()),
        })
        .collect()
}
