use libmandoc_rs::structured::{
    ContentAtom, ContentAtomKey, ContentAtomKind, ContentPointKey, ContentRootKey,
    LinkOccurrenceKey, PointBoundary, ProvenanceKey, StructuredStyle,
};
use mant_ir::{Diagnostic, DiagnosticImpact, DiagnosticLevel, Inline, Provenance, SourceSpan};

use super::{NativeProjectionError, NativeProseProjection, address::AddressPlan};

type PendingLink = Option<(LinkOccurrenceKey, Vec<Inline>)>;
type InAtomPoints = std::collections::HashMap<ContentAtomKey, Vec<(u32, u32, ContentPointKey)>>;

struct PointBuckets {
    between: Vec<Vec<ContentPointKey>>,
    inside: InAtomPoints,
}

pub(super) fn root_inlines(
    projection: &NativeProseProjection,
    addresses: &AddressPlan,
    root: ContentRootKey,
) -> Result<Vec<Inline>, NativeProjectionError> {
    let document = projection.document();
    let projected = projection
        .root(root)
        .ok_or(NativeProjectionError::InvalidRelation(
            "content root has no projected leaves",
        ))?;
    let points = collect_point_buckets(document, addresses, root, projected.leaves.len())?;
    let mut inlines = Vec::new();
    let mut pending_link = None;

    for (leaf_index, leaf) in projected.leaves.iter().enumerate() {
        let atom =
            document
                .content_atom(leaf.atom())
                .ok_or(NativeProjectionError::InvalidRelation(
                    "projected leaf references an unknown atom",
                ))?;
        let continuation = match atom.kind() {
            ContentAtomKind::BreakOpportunity => {
                next_visible_link(document, addresses, projected, leaf_index + 1)?
            }
            ContentAtomKind::Text { .. }
            | ContentAtomKind::Whitespace { .. }
            | ContentAtomKind::HardBreak => retained_link(addresses, atom.link())?,
        };
        emit_points(
            document,
            addresses,
            &points.between[leaf_index],
            continuation,
            &mut pending_link,
            &mut inlines,
        )?;

        match atom.kind() {
            ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
                lower_text_atom(
                    document,
                    addresses,
                    atom,
                    text,
                    points
                        .inside
                        .get(&atom.key())
                        .map_or(&[][..], Vec::as_slice),
                    &mut pending_link,
                    &mut inlines,
                )?;
            }
            ContentAtomKind::HardBreak => push_leaf(
                document,
                addresses,
                retained_link(addresses, atom.link())?,
                styled_leaf(atom, Inline::LineBreak),
                &mut pending_link,
                &mut inlines,
            )?,
            ContentAtomKind::BreakOpportunity => {}
        }
    }
    emit_points(
        document,
        addresses,
        &points.between[projected.leaves.len()],
        None,
        &mut pending_link,
        &mut inlines,
    )?;
    flush_link(document, addresses, &mut pending_link, &mut inlines)?;
    Ok(inlines)
}

fn collect_point_buckets(
    document: &libmandoc_rs::structured::StructuredDocument,
    addresses: &AddressPlan,
    root: ContentRootKey,
    atom_count: usize,
) -> Result<PointBuckets, NativeProjectionError> {
    let mut buckets = PointBuckets {
        between: vec![Vec::new(); atom_count + 1],
        inside: InAtomPoints::new(),
    };
    for point in document
        .content_points()
        .iter()
        .filter(|point| point.root() == root && addresses.anchor(point.key()).is_some())
    {
        match point.boundary() {
            PointBoundary::BetweenAtoms { atom_boundary } => buckets
                .between
                .get_mut(*atom_boundary as usize)
                .ok_or(NativeProjectionError::InvalidRelation(
                    "content point atom boundary is outside its root",
                ))?
                .push(point.key()),
            PointBoundary::InAtom { atom, byte_offset } => buckets
                .inside
                .entry(*atom)
                .or_default()
                .push((*byte_offset, point.ordinal(), point.key())),
        }
    }
    for points in buckets.inside.values_mut() {
        points.sort_unstable();
    }
    Ok(buckets)
}

fn lower_text_atom(
    document: &libmandoc_rs::structured::StructuredDocument,
    addresses: &AddressPlan,
    atom: &ContentAtom,
    text: &str,
    points: &[(u32, u32, ContentPointKey)],
    pending: &mut PendingLink,
    output: &mut Vec<Inline>,
) -> Result<(), NativeProjectionError> {
    let link = retained_link(addresses, atom.link())?;
    let mut fragments = Vec::with_capacity(points.len().saturating_mul(2).saturating_add(1));
    let mut start = 0_usize;
    let mut point_index = 0;
    while point_index < points.len() {
        let byte_offset = points[point_index].0;
        let end = byte_offset as usize;
        let value = text
            .get(start..end)
            .ok_or(NativeProjectionError::InvalidRelation(
                "content point does not select a UTF-8 atom boundary",
            ))?;
        push_text_fragment(&mut fragments, atom.style(), value);
        let group_start = point_index;
        while point_index < points.len() && points[point_index].0 == byte_offset {
            point_index += 1;
        }
        fragments.extend(
            points[group_start..point_index]
                .iter()
                .filter_map(|(_, _, key)| addresses.anchor(*key))
                .map(super::address::AnchorPlacement::inline),
        );
        start = end;
    }
    let tail = text
        .get(start..)
        .ok_or(NativeProjectionError::InvalidRelation(
            "content point tail is outside its atom",
        ))?;
    push_text_fragment(&mut fragments, atom.style(), tail);
    for fragment in wrap_atom_style(atom.style(), fragments) {
        push_leaf(document, addresses, link, fragment, pending, output)?;
    }
    Ok(())
}

fn push_text_fragment(fragments: &mut Vec<Inline>, style: StructuredStyle, value: &str) {
    if value.is_empty() {
        return;
    }
    fragments.push(if style.is_literal() {
        Inline::Code {
            value: value.to_owned(),
        }
    } else {
        Inline::Text {
            value: value.to_owned(),
        }
    });
}

fn next_visible_link(
    document: &libmandoc_rs::structured::StructuredDocument,
    addresses: &AddressPlan,
    root: &super::super::projection::NativeProseRoot,
    start: usize,
) -> Result<Option<LinkOccurrenceKey>, NativeProjectionError> {
    let leaves = root
        .leaves
        .get(start..)
        .ok_or(NativeProjectionError::InvalidRelation(
            "projected leaf index is out of bounds",
        ))?;
    for leaf in leaves {
        let atom =
            document
                .content_atom(leaf.atom())
                .ok_or(NativeProjectionError::InvalidRelation(
                    "projected leaf references an unknown atom",
                ))?;
        match atom.kind() {
            ContentAtomKind::BreakOpportunity | ContentAtomKind::HardBreak => {}
            ContentAtomKind::Text { .. } | ContentAtomKind::Whitespace { .. } => {
                return retained_link(addresses, atom.link());
            }
        }
    }
    Ok(None)
}

fn retained_link(
    addresses: &AddressPlan,
    link: Option<LinkOccurrenceKey>,
) -> Result<Option<LinkOccurrenceKey>, NativeProjectionError> {
    link.map(|key| addresses.link_target(key).map(|target| target.map(|_| key)))
        .transpose()
        .map(Option::flatten)
}

fn styled_leaf(atom: &ContentAtom, leaf: Inline) -> Inline {
    wrap_atom_style(atom.style(), vec![leaf])
        .pop()
        .expect("one styled leaf remains one inline")
}

fn wrap_atom_style(style: StructuredStyle, mut fragments: Vec<Inline>) -> Vec<Inline> {
    if style.is_italic() {
        fragments = vec![Inline::Emphasis {
            children: fragments,
        }];
    }
    if style.is_bold() || style.is_underline() {
        fragments = vec![Inline::Strong {
            children: fragments,
        }];
    }
    fragments
}

fn push_leaf(
    document: &libmandoc_rs::structured::StructuredDocument,
    addresses: &AddressPlan,
    link: Option<LinkOccurrenceKey>,
    leaf: Inline,
    pending: &mut Option<(LinkOccurrenceKey, Vec<Inline>)>,
    output: &mut Vec<Inline>,
) -> Result<(), NativeProjectionError> {
    if let (Some((pending_key, children)), Some(link)) = (pending.as_mut(), link)
        && *pending_key == link
    {
        children.push(leaf);
        return Ok(());
    }
    flush_link(document, addresses, pending, output)?;
    if let Some(link) = link {
        *pending = Some((link, vec![leaf]));
    } else {
        output.push(leaf);
    }
    Ok(())
}

fn emit_points(
    document: &libmandoc_rs::structured::StructuredDocument,
    addresses: &AddressPlan,
    points: &[ContentPointKey],
    continuation: Option<LinkOccurrenceKey>,
    pending: &mut Option<(LinkOccurrenceKey, Vec<Inline>)>,
    output: &mut Vec<Inline>,
) -> Result<(), NativeProjectionError> {
    if pending.as_ref().map(|(key, _)| *key) != continuation {
        flush_link(document, addresses, pending, output)?;
    }
    for point in points {
        let Some(anchor) = addresses.anchor(*point) else {
            continue;
        };
        let anchor = anchor.inline();
        if let Some((key, children)) = pending.as_mut()
            && Some(*key) == continuation
        {
            children.push(anchor);
        } else {
            output.push(anchor);
        }
    }
    Ok(())
}

fn flush_link(
    document: &libmandoc_rs::structured::StructuredDocument,
    addresses: &AddressPlan,
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
    if let Some(target) = addresses.link_target(key)? {
        output.push(Inline::Link {
            target: target.clone(),
            title: link.title().map(ToOwned::to_owned),
            children,
        });
    } else {
        output.extend(children);
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use libmandoc_rs::structured::StructuredStyle;
    use mant_ir::{FragmentAlias, Inline, NodeId};

    use super::wrap_atom_style;

    #[test]
    fn in_atom_point_keeps_one_style_wrapper_around_both_text_fragments() {
        let fragments = vec![
            Inline::Text {
                value: "before".to_owned(),
            },
            Inline::Anchor {
                id: NodeId::new("point"),
                fragment_aliases: vec![FragmentAlias::from("Point")],
                owner_source: None,
            },
            Inline::Text {
                value: "after".to_owned(),
            },
        ];
        let styled = wrap_atom_style(StructuredStyle::BoldItalic, fragments);

        let [Inline::Strong { children }] = styled.as_slice() else {
            panic!("bold style must have one outer wrapper: {styled:#?}");
        };
        let [Inline::Emphasis { children }] = children.as_slice() else {
            panic!("italic style must have one inner wrapper: {children:#?}");
        };
        assert!(
            matches!(children.as_slice(), [Inline::Text { value: before }, Inline::Anchor { id, .. }, Inline::Text { value: after }] if before == "before" && id.as_str() == "point" && after == "after")
        );
    }
}
