use libmandoc_rs::structured::{
    ContentAtomKind, ContentRootKey, LinkOccurrenceKey, NativeLinkTarget, ProvenanceKey,
};
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
    let mut pending_link: Option<(LinkOccurrenceKey, Vec<Inline>)> = None;
    let mut lookahead_boundary = 0;
    let mut lookahead_link = None;
    for (leaf_index, leaf) in projected.leaves.iter().enumerate() {
        let atom =
            document
                .content_atom(leaf.atom())
                .ok_or(NativeProjectionError::InvalidRelation(
                    "projected leaf references an unknown atom",
                ))?;
        let (leaf, link) = match atom.kind() {
            ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
                let leaf = if atom.style().is_literal() {
                    Inline::Code {
                        value: text.clone(),
                    }
                } else {
                    Inline::Text {
                        value: text.clone(),
                    }
                };
                (leaf, atom.link())
            }
            ContentAtomKind::BreakOpportunity => {
                // Break opportunities are logical formatter boundaries, not inline
                // wrapper boundaries.  In particular, a native partial flush must
                // not split the surrounding link occurrence.
                continue;
            }
            ContentAtomKind::HardBreak => {
                if leaf_index >= lookahead_boundary {
                    (lookahead_boundary, lookahead_link) =
                        next_visible_link(document, projected, leaf_index + 1)?;
                }
                let continued_link = pending_link
                    .as_ref()
                    .map(|(key, _)| *key)
                    .filter(|key| lookahead_link == Some(*key));
                (Inline::LineBreak, continued_link)
            }
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
        match (pending_link.as_mut(), link) {
            (Some((pending, children)), Some(link)) if *pending == link => {
                children.push(leaf);
            }
            (_, link) => {
                flush_link(document, &mut pending_link, &mut inlines)?;
                if let Some(link) = link {
                    pending_link = Some((link, vec![leaf]));
                } else {
                    inlines.push(leaf);
                }
            }
        }
    }
    flush_link(document, &mut pending_link, &mut inlines)?;
    Ok(inlines)
}

fn next_visible_link(
    document: &libmandoc_rs::structured::StructuredDocument,
    root: &super::super::projection::NativeProseRoot,
    start: usize,
) -> Result<(usize, Option<LinkOccurrenceKey>), NativeProjectionError> {
    let leaves = root
        .leaves
        .get(start..)
        .ok_or(NativeProjectionError::InvalidRelation(
            "projected leaf index is out of bounds",
        ))?;
    for (offset, leaf) in leaves.iter().enumerate() {
        let atom =
            document
                .content_atom(leaf.atom())
                .ok_or(NativeProjectionError::InvalidRelation(
                    "projected leaf references an unknown atom",
                ))?;
        match atom.kind() {
            ContentAtomKind::BreakOpportunity | ContentAtomKind::HardBreak => {}
            ContentAtomKind::Text { .. } | ContentAtomKind::Whitespace { .. } => {
                return Ok((start + offset, atom.link()));
            }
        }
    }
    Ok((root.leaves.len(), None))
}

fn flush_link(
    document: &libmandoc_rs::structured::StructuredDocument,
    pending: &mut Option<(LinkOccurrenceKey, Vec<Inline>)>,
    output: &mut Vec<Inline>,
) -> Result<(), NativeProjectionError> {
    let Some((key, children)) = pending.take() else {
        return Ok(());
    };
    let link = document
        .link(key)
        .ok_or(NativeProjectionError::InvalidRelation(
            "atom references an unknown link",
        ))?;
    output.push(Inline::Link {
        target: lower_link(link.target()),
        title: link.title().map(ToOwned::to_owned),
        children,
    });
    Ok(())
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
