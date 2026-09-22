use libmandoc_rs::structured::{ContentAtomKey, ContentPointKey, ContentRootKey, ProvenanceKey};
use mant_ir::{
    ContentAtom, ContentAtomKind, ContentByteRange, ContentRef, ContentStyle, Diagnostic,
    DiagnosticImpact, DiagnosticLevel, Inline, LinkOccurrenceKey, PointBoundary, Provenance,
    SourceSpan,
};

use super::{
    NativeProjectionError, NativeProseProjection, address::AddressPlan, store::NativeContentMap,
};

type PendingLink = Option<(LinkOccurrenceKey, Vec<Inline>)>;
type InAtomPoints = std::collections::HashMap<ContentAtomKey, Vec<(u32, u32, ContentPointKey)>>;

struct PointBuckets {
    between: Vec<Vec<ContentPointKey>>,
    inside: InAtomPoints,
}

pub(super) fn root_inlines(
    projection: &NativeProseProjection,
    addresses: &AddressPlan,
    content: &NativeContentMap,
    root: ContentRootKey,
) -> Result<Vec<Inline>, NativeProjectionError> {
    let projected = projection
        .root(root)
        .ok_or(NativeProjectionError::InvalidRelation(
            "content root has no projected leaves",
        ))?;
    let public_root = content.root(root)?;
    let points = collect_point_buckets(content, addresses, public_root, projected.leaves.len())?;
    let mut inlines = Vec::new();
    let mut pending_link = None;

    for (leaf_index, leaf) in projected.leaves.iter().enumerate() {
        let native_atom = leaf.atom();
        let atom = public_atom(content, native_atom)?;
        let continuation = match &atom.kind {
            ContentAtomKind::BreakOpportunity {} => {
                next_visible_link(content, projected, leaf_index + 1)?
            }
            ContentAtomKind::Text { .. }
            | ContentAtomKind::Whitespace { .. }
            | ContentAtomKind::HardBreak {} => atom.link,
        };
        emit_points(
            content,
            addresses,
            &points.between[leaf_index],
            continuation,
            &mut pending_link,
            &mut inlines,
        )?;

        match &atom.kind {
            ContentAtomKind::Text { text, .. } | ContentAtomKind::Whitespace { text, .. } => {
                lower_text_atom(
                    content,
                    addresses,
                    atom,
                    text,
                    points
                        .inside
                        .get(&native_atom)
                        .map_or(&[][..], Vec::as_slice),
                    &mut pending_link,
                    &mut inlines,
                )?;
            }
            ContentAtomKind::HardBreak {} => push_leaf(
                atom.link,
                styled_leaf(atom, Inline::LineBreak { atom: atom.key }),
                &mut pending_link,
                &mut inlines,
            ),
            ContentAtomKind::BreakOpportunity {} => {}
        }
    }
    emit_points(
        content,
        addresses,
        &points.between[projected.leaves.len()],
        None,
        &mut pending_link,
        &mut inlines,
    )?;
    flush_link(&mut pending_link, &mut inlines);
    Ok(inlines)
}

fn collect_point_buckets(
    content: &NativeContentMap,
    addresses: &AddressPlan,
    root: mant_ir::ContentRootKey,
    atom_count: usize,
) -> Result<PointBuckets, NativeProjectionError> {
    let mut buckets = PointBuckets {
        between: vec![Vec::new(); atom_count + 1],
        inside: InAtomPoints::new(),
    };
    let root_record = content
        .store()
        .root(root)
        .ok_or(NativeProjectionError::InvalidRelation(
            "public content root is missing",
        ))?;
    for public_key in &root_record.points {
        let point =
            content
                .store()
                .point(*public_key)
                .ok_or(NativeProjectionError::InvalidRelation(
                    "public content point is missing",
                ))?;
        let native_key = content.native_point(*public_key)?;
        if addresses.anchor(native_key).is_none() {
            continue;
        }
        match point.boundary {
            PointBoundary::BetweenAtoms { atom_boundary } => buckets
                .between
                .get_mut(atom_boundary as usize)
                .ok_or(NativeProjectionError::InvalidRelation(
                    "content point atom boundary is outside its root",
                ))?
                .push(native_key),
            PointBoundary::InAtom { atom, byte_offset } => buckets
                .inside
                .entry(content.native_atom(atom)?)
                .or_default()
                .push((byte_offset, public_key.get(), native_key)),
        }
    }
    for points in buckets.inside.values_mut() {
        points.sort_unstable();
    }
    Ok(buckets)
}

fn lower_text_atom(
    content: &NativeContentMap,
    addresses: &AddressPlan,
    atom: &mant_ir::ContentAtom,
    text: &str,
    points: &[(u32, u32, ContentPointKey)],
    pending: &mut PendingLink,
    output: &mut Vec<Inline>,
) -> Result<(), NativeProjectionError> {
    let link = atom.link;
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
        push_text_fragment(&mut fragments, atom, start, end, value);
        let group_start = point_index;
        while point_index < points.len() && points[point_index].0 == byte_offset {
            point_index += 1;
        }
        for (_, _, key) in &points[group_start..point_index] {
            if let Some(anchor) = addresses.anchor(*key) {
                fragments.push(anchor.inline(content.point(*key)?));
            }
        }
        start = end;
    }
    let tail = text
        .get(start..)
        .ok_or(NativeProjectionError::InvalidRelation(
            "content point tail is outside its atom",
        ))?;
    push_text_fragment(&mut fragments, atom, start, text.len(), tail);
    for fragment in wrap_atom_style(atom.style, fragments) {
        push_leaf(link, fragment, pending, output);
    }
    Ok(())
}

fn push_text_fragment(
    fragments: &mut Vec<Inline>,
    atom: &ContentAtom,
    start: usize,
    end: usize,
    value: &str,
) {
    if value.is_empty() {
        return;
    }
    let content = ContentRef {
        atom: atom.key,
        bytes: ContentByteRange {
            start: u32::try_from(start).expect("validated native atom byte range fits u32"),
            end: u32::try_from(end).expect("validated native atom byte range fits u32"),
        },
    };
    fragments.push(if atom.style.literal {
        Inline::Code { content }
    } else {
        Inline::Text { content }
    });
}

fn next_visible_link(
    content: &NativeContentMap,
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
        let atom = public_atom(content, leaf.atom())?;
        match &atom.kind {
            ContentAtomKind::BreakOpportunity {} | ContentAtomKind::HardBreak {} => {}
            ContentAtomKind::Text { .. } | ContentAtomKind::Whitespace { .. } => {
                return Ok(atom.link);
            }
        }
    }
    Ok(None)
}

fn styled_leaf(atom: &ContentAtom, leaf: Inline) -> Inline {
    wrap_atom_style(atom.style, vec![leaf])
        .pop()
        .expect("one styled leaf remains one inline")
}

fn wrap_atom_style(style: ContentStyle, mut fragments: Vec<Inline>) -> Vec<Inline> {
    if style.emphasis {
        fragments = vec![Inline::Emphasis {
            children: fragments,
        }];
    }
    if style.strong || style.underline {
        fragments = vec![Inline::Strong {
            children: fragments,
        }];
    }
    fragments
}

fn push_leaf(
    link: Option<LinkOccurrenceKey>,
    leaf: Inline,
    pending: &mut Option<(LinkOccurrenceKey, Vec<Inline>)>,
    output: &mut Vec<Inline>,
) {
    if let (Some((pending_key, children)), Some(link)) = (pending.as_mut(), link)
        && *pending_key == link
    {
        children.push(leaf);
        return;
    }
    flush_link(pending, output);
    if let Some(link) = link {
        *pending = Some((link, vec![leaf]));
    } else {
        output.push(leaf);
    }
}

fn emit_points(
    content: &NativeContentMap,
    addresses: &AddressPlan,
    points: &[ContentPointKey],
    continuation: Option<LinkOccurrenceKey>,
    pending: &mut Option<(LinkOccurrenceKey, Vec<Inline>)>,
    output: &mut Vec<Inline>,
) -> Result<(), NativeProjectionError> {
    if pending.as_ref().map(|(key, _)| *key) != continuation {
        flush_link(pending, output);
    }
    for point in points {
        let Some(anchor) = addresses.anchor(*point) else {
            continue;
        };
        let anchor = anchor.inline(content.point(*point)?);
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

fn flush_link(pending: &mut Option<(LinkOccurrenceKey, Vec<Inline>)>, output: &mut Vec<Inline>) {
    let Some((key, children)) = pending.take() else {
        return;
    };
    output.push(Inline::Link {
        occurrence: key,
        children,
    });
}

fn public_atom(
    content: &NativeContentMap,
    key: ContentAtomKey,
) -> Result<&ContentAtom, NativeProjectionError> {
    content
        .store()
        .atom(content.atom(key)?)
        .ok_or(NativeProjectionError::InvalidRelation(
            "public content atom is missing",
        ))
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
    use mant_ir::{
        ContentAtomKey, ContentByteRange, ContentPointKey, ContentRef, ContentStyle, FragmentAlias,
        Inline, NodeId,
    };

    use super::wrap_atom_style;

    #[test]
    fn in_atom_point_keeps_one_style_wrapper_around_both_text_fragments() {
        let fragments = vec![
            Inline::Text {
                content: content(0, 6),
            },
            Inline::Anchor {
                point: ContentPointKey::FIRST,
                id: NodeId::new("point"),
                fragment_aliases: vec![FragmentAlias::from("Point")],
            },
            Inline::Text {
                content: content(6, 11),
            },
        ];
        let styled = wrap_atom_style(
            ContentStyle {
                strong: true,
                emphasis: true,
                literal: false,
                underline: false,
            },
            fragments,
        );

        let [Inline::Strong { children }] = styled.as_slice() else {
            panic!("bold style must have one outer wrapper: {styled:#?}");
        };
        let [Inline::Emphasis { children }] = children.as_slice() else {
            panic!("italic style must have one inner wrapper: {children:#?}");
        };
        assert!(
            matches!(children.as_slice(), [Inline::Text { content: before }, Inline::Anchor { id, .. }, Inline::Text { content: after }] if before.bytes == ContentByteRange { start: 0, end: 6 } && id.as_str() == "point" && after.bytes == ContentByteRange { start: 6, end: 11 })
        );
    }

    fn content(start: u32, end: u32) -> ContentRef {
        ContentRef {
            atom: ContentAtomKey::FIRST,
            bytes: ContentByteRange { start, end },
        }
    }
}
