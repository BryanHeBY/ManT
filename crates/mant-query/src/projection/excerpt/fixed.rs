//! Fixed structural excerpts from the one validated native display surface.

use std::collections::HashSet;

use mant_ir::{ContentReveal, FixedBody, FixedSectionReader, OutlinePath, OutputSlice};
use mant_protocol::{
    ContentSelector, EntryProjection, ExcerptSchema, ExcerptSelection, FixedExcerptPart,
    FixedExcerptSelection, MAX_FIXED_EXCERPT_BYTES, MAX_FIXED_EXCERPT_PARTS, OutlineNode,
    OutlineNodeReference, OutlineReference, OutlineTrail, QueryExcerpt,
};
use unicode_width::UnicodeWidthChar;

use super::{ProjectionError, ResolvedContent, semantics_complete};

struct Candidate<'a> {
    ordinal: usize,
    node: &'a OutlineNode,
    ancestors: Vec<OutlineReference>,
}

/// Resolve outline selectors before copying any native body bytes. Selected
/// ancestors suppress descendants, so one response never copies a subtree
/// merely because a child was also requested.
pub(super) fn select_fixed_excerpt(
    query: &ResolvedContent,
    fixed: &FixedBody,
    selectors: &[ContentSelector],
) -> Result<QueryExcerpt, ProjectionError> {
    let reader = FixedSectionReader::new(fixed).map_err(|_| ProjectionError::ContentProjection)?;
    let outline =
        super::super::outline::build_outline_projection(query, EntryProjection::All, None)?;
    let mut candidates = Vec::new();
    collect_candidates(&outline.nodes, &[], &mut candidates);
    let selected = resolve_candidates(query, selectors, &candidates)?;
    let only_tldr = selected
        .iter()
        .all(|candidate| matches!(candidate.node, OutlineNode::Tldr { .. }));
    let document = if only_tldr {
        None
    } else {
        query.document.as_ref()
    };
    let mut selections = Vec::new();
    let mut total_parts = 0usize;
    let mut total_bytes = 0u64;
    for candidate in selected {
        let remaining_parts = MAX_FIXED_EXCERPT_PARTS
            .checked_sub(total_parts)
            .ok_or(ProjectionError::ContentProjection)?;
        let remaining_bytes = MAX_FIXED_EXCERPT_BYTES
            .checked_sub(total_bytes)
            .ok_or(ProjectionError::ContentProjection)?;
        let selection = materialize_selection(
            query,
            fixed,
            &reader,
            candidate,
            remaining_parts,
            remaining_bytes,
        )?;
        if let Some(view) = fixed_view(&selection) {
            total_parts = total_parts
                .checked_add(view.parts.len())
                .ok_or(ProjectionError::ContentProjection)?;
            total_bytes = total_bytes
                .checked_add(
                    view.budget_use()
                        .map_err(|_| ProjectionError::ContentProjection)?,
                )
                .ok_or(ProjectionError::ContentProjection)?;
            if total_parts > MAX_FIXED_EXCERPT_PARTS || total_bytes > MAX_FIXED_EXCERPT_BYTES {
                return Err(ProjectionError::ContentProjection);
            }
        }
        selections.push(selection);
    }
    let diagnostics = document.map_or_else(Vec::new, mant_ir::Document::projection_diagnostics);
    Ok(QueryExcerpt {
        display_title: query
            .document
            .as_ref()
            .and_then(mant_ir::Document::display_title)
            .map(std::borrow::Cow::into_owned),
        schema: ExcerptSchema::V0Dot12,
        label: query.label.clone(),
        address: query.address.clone(),
        semantics_complete: semantics_complete(&diagnostics),
        producer: document.map(mant_protocol::Producer::for_document),
        source_context: document.map(mant_protocol::SourceContext::from),
        meta: document.map(|document| document.meta.clone()),
        diagnostics,
        content_projection: None,
        selections,
    })
}

fn resolve_candidates<'a, 'b>(
    query: &ResolvedContent,
    selectors: &[ContentSelector],
    candidates: &'b [Candidate<'a>],
) -> Result<Vec<&'b Candidate<'a>>, ProjectionError> {
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for selector in selectors {
        let matches = candidates
            .iter()
            .filter(|candidate| match selector {
                ContentSelector::Path { path } => candidate.node.path() == path.as_str(),
                ContentSelector::Id { id } => candidate.node.id() == id.as_str(),
            })
            .collect::<Vec<_>>();
        let candidate = match matches.as_slice() {
            [candidate] => *candidate,
            [] => {
                return Err(ProjectionError::UnknownSelector {
                    document: query.label.clone(),
                    selector: selector.to_string(),
                });
            }
            matches => {
                return Err(ProjectionError::AmbiguousSelector {
                    document: query.label.clone(),
                    selector: selector.to_string(),
                    candidates: matches
                        .iter()
                        .map(|candidate| crate::selectors::SelectorCandidate {
                            path: candidate.node.path().to_owned(),
                            id: candidate.node.id().to_owned(),
                        })
                        .collect(),
                });
            }
        };
        if seen.insert(candidate.node.path()) {
            selected.push(candidate);
        }
    }
    let selected_paths = selected
        .iter()
        .map(|candidate| candidate.node.path())
        .collect::<Vec<_>>();
    selected.retain(|candidate| {
        !selected_paths.iter().any(|ancestor| {
            *ancestor != candidate.node.path() && dominates(ancestor, candidate.node.path())
        })
    });
    selected.sort_by_key(|candidate| candidate.ordinal);
    Ok(selected)
}

fn materialize_selection(
    query: &ResolvedContent,
    fixed: &FixedBody,
    reader: &FixedSectionReader<'_>,
    candidate: &Candidate<'_>,
    remaining_parts: usize,
    remaining_bytes: u64,
) -> Result<ExcerptSelection, ProjectionError> {
    let outline = trail(candidate.node, &candidate.ancestors);
    match candidate.node {
        OutlineNode::Tldr { .. } => Ok(ExcerptSelection::Tldr {
            outline,
            document: query
                .tldr
                .clone()
                .ok_or(ProjectionError::ContentProjection)?,
        }),
        OutlineNode::DocumentRoot { .. } => Ok(ExcerptSelection::FixedDocumentRoot {
            outline,
            view: copy_parts(
                fixed,
                reader
                    .root_preface_parts()
                    .map_err(|_| ProjectionError::ContentProjection)?
                    .iter()
                    .map(|part| part.slice),
                remaining_parts,
                remaining_bytes,
            )?,
        }),
        OutlineNode::DocumentSection { path, .. } => {
            let path: OutlinePath = path
                .parse()
                .map_err(|_| ProjectionError::ContentProjection)?;
            let heading = reader
                .section_at(&path)
                .ok_or(ProjectionError::ContentProjection)?;
            let parts = reader
                .subtree_parts(heading.key)
                .ok_or(ProjectionError::ContentProjection)?;
            Ok(ExcerptSelection::FixedDocumentSection {
                outline,
                view: copy_parts(
                    fixed,
                    parts.iter().map(|part| part.slice),
                    remaining_parts,
                    remaining_bytes,
                )?,
            })
        }
        OutlineNode::DocumentEntry { owner, .. } => {
            let ContentReveal::FixedOwner { key } = owner.as_ref() else {
                return Err(ProjectionError::ContentProjection);
            };
            let mark = fixed
                .owners
                .get((key.get() - 1) as usize)
                .ok_or(ProjectionError::ContentProjection)?;
            let body = reader
                .owner_body_parts(*key)
                .ok_or(ProjectionError::ContentProjection)?;
            let head = mark.head.parts.iter().copied();
            Ok(ExcerptSelection::FixedDocumentEntry {
                outline,
                view: copy_parts(
                    fixed,
                    head.chain(body.iter().map(|part| part.slice)),
                    remaining_parts,
                    remaining_bytes,
                )?,
            })
        }
    }
}

fn dominates(parent: &str, child: &str) -> bool {
    child
        .strip_prefix(parent)
        .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('/'))
}

fn collect_candidates<'a>(
    nodes: &'a [OutlineNode],
    ancestors: &[OutlineReference],
    output: &mut Vec<Candidate<'a>>,
) {
    for node in nodes {
        let ordinal = output.len();
        output.push(Candidate {
            ordinal,
            node,
            ancestors: ancestors.to_vec(),
        });
        let mut children_ancestors = ancestors.to_vec();
        children_ancestors.push(OutlineReference {
            path: node.path().to_owned().into(),
            id: node.id().to_owned().into(),
            title: node.title().to_owned(),
        });
        collect_candidates(node.children(), &children_ancestors, output);
    }
}

fn trail(node: &OutlineNode, ancestors: &[OutlineReference]) -> OutlineTrail {
    let reference = match node {
        OutlineNode::Tldr { path, id, title } => OutlineNodeReference::Tldr {
            path: path.clone(),
            id: id.clone(),
            title: title.clone(),
        },
        OutlineNode::DocumentRoot {
            path, id, title, ..
        } => OutlineNodeReference::DocumentRoot {
            path: path.clone(),
            id: id.clone(),
            title: title.clone(),
        },
        OutlineNode::DocumentSection {
            path, id, title, ..
        } => OutlineNodeReference::DocumentSection {
            path: path.clone(),
            id: id.clone(),
            title: title.clone(),
        },
        OutlineNode::DocumentEntry {
            path,
            id,
            title,
            entry_kind,
            case,
            names,
            ..
        } => OutlineNodeReference::DocumentEntry {
            path: path.clone(),
            id: id.clone(),
            title: title.clone(),
            entry_kind: *entry_kind,
            case: *case,
            names: names.clone(),
        },
    };
    OutlineTrail {
        ancestors: ancestors.to_vec(),
        node: reference,
    }
}

fn fixed_view(selection: &ExcerptSelection) -> Option<&FixedExcerptSelection> {
    match selection {
        ExcerptSelection::FixedDocumentRoot { view, .. }
        | ExcerptSelection::FixedDocumentSection { view, .. }
        | ExcerptSelection::FixedDocumentEntry { view, .. } => Some(view),
        _ => None,
    }
}

fn copy_parts(
    fixed: &FixedBody,
    slices: impl IntoIterator<Item = OutputSlice>,
    remaining_parts: usize,
    remaining_bytes: u64,
) -> Result<FixedExcerptSelection, ProjectionError> {
    let mut selected = Vec::new();
    let mut bytes = 0u64;
    for slice in slices {
        if selected.len() >= remaining_parts {
            return Err(ProjectionError::ContentProjection);
        }
        bytes = bytes
            .checked_add(
                slice
                    .end_byte
                    .checked_sub(slice.start_byte)
                    .filter(|length| *length > 0)
                    .ok_or(ProjectionError::ContentProjection)?,
            )
            .ok_or(ProjectionError::ContentProjection)?;
        if bytes > remaining_bytes {
            // Check before cloning any display text. A native surface can
            // contain far more UTF-8 than the bounded response may return.
            return Err(ProjectionError::ContentProjection);
        }
        selected.push(slice);
    }
    // DisplaySurface::validate proves dense run keys in physical row/column
    // order; a run-relative byte offset then refines that same display order.
    selected.sort_by_key(|slice| (slice.run, slice.start_byte));
    let mut parts = Vec::with_capacity(selected.len());
    let mut index = 0;
    while index < selected.len() {
        let run_key = selected[index].run;
        let mut end = index + 1;
        while end < selected.len() && selected[end].run == run_key {
            end += 1;
        }
        copy_run_parts(fixed, &selected[index..end], &mut parts)?;
        index = end;
    }
    let view = FixedExcerptSelection { parts };
    if view
        .budget_use()
        .map_err(|_| ProjectionError::ContentProjection)?
        > remaining_bytes
    {
        return Err(ProjectionError::ContentProjection);
    }
    Ok(view)
}

/// Build a cell index for only the selected boundaries in one final run.
/// Repeated partial slices must not rescan the whole run for every prefix.
fn copy_run_parts(
    fixed: &FixedBody,
    slices: &[OutputSlice],
    parts: &mut Vec<FixedExcerptPart>,
) -> Result<(), ProjectionError> {
    let run_key = slices
        .first()
        .ok_or(ProjectionError::ContentProjection)?
        .run;
    let run = fixed
        .surface
        .runs
        .get((run_key.get() - 1) as usize)
        .ok_or(ProjectionError::ContentProjection)?;
    let whole = fixed
        .surface
        .run_text(run_key)
        .ok_or(ProjectionError::ContentProjection)?;
    let whole_slice =
        slices.len() == 1 && slices[0].start_byte == 0 && slices[0].end_byte == whole.len() as u64;
    let mut boundaries = Vec::new();
    let cells = if whole_slice {
        Vec::new()
    } else {
        // Pinned term_ascii.c::utf8_getwidth uses per-scalar native widths.
        // A clipped run is admitted only if that mapping agrees with the
        // native whole-run width; byte offsets never become cell columns.
        for slice in slices {
            boundaries.push(
                usize::try_from(slice.start_byte)
                    .map_err(|_| ProjectionError::ContentProjection)?,
            );
            boundaries.push(
                usize::try_from(slice.end_byte).map_err(|_| ProjectionError::ContentProjection)?,
            );
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        native_widths_at(whole, run.width, &boundaries)?
    };
    let mut previous_end = 0u64;
    for slice in slices {
        if slice.start_byte < previous_end {
            return Err(ProjectionError::ContentProjection);
        }
        previous_end = slice.end_byte;
        let start =
            usize::try_from(slice.start_byte).map_err(|_| ProjectionError::ContentProjection)?;
        let end =
            usize::try_from(slice.end_byte).map_err(|_| ProjectionError::ContentProjection)?;
        let text = whole
            .get(start..end)
            .ok_or(ProjectionError::ContentProjection)?;
        let (column, width) = if whole_slice {
            (run.column, run.width)
        } else {
            let start_index = boundaries
                .binary_search(&start)
                .map_err(|_| ProjectionError::ContentProjection)?;
            let end_index = boundaries
                .binary_search(&end)
                .map_err(|_| ProjectionError::ContentProjection)?;
            let start_cell = cells[start_index];
            let end_cell = cells[end_index];
            (
                run.column
                    .checked_add(start_cell)
                    .ok_or(ProjectionError::ContentProjection)?,
                end_cell
                    .checked_sub(start_cell)
                    .ok_or(ProjectionError::ContentProjection)?,
            )
        };
        parts.push(FixedExcerptPart {
            slice: *slice,
            row: run.row,
            column,
            width,
            style: run.label.style,
            text: text.to_owned(),
            source: run.label.source,
        });
    }
    Ok(())
}

fn native_widths_at(
    text: &str,
    native_width: u32,
    boundaries: &[usize],
) -> Result<Vec<u32>, ProjectionError> {
    if boundaries
        .iter()
        .any(|&byte| byte > text.len() || !text.is_char_boundary(byte))
    {
        return Err(ProjectionError::ContentProjection);
    }
    let mut widths = Vec::with_capacity(boundaries.len());
    let mut next = 0usize;
    let mut cells = 0u32;
    for (byte, scalar) in text.char_indices() {
        while boundaries.get(next) == Some(&byte) {
            widths.push(cells);
            next += 1;
        }
        cells = cells
            .checked_add(
                u32::try_from(scalar.width().unwrap_or(0))
                    .map_err(|_| ProjectionError::ContentProjection)?,
            )
            .ok_or(ProjectionError::ContentProjection)?;
    }
    while boundaries.get(next) == Some(&text.len()) {
        widths.push(cells);
        next += 1;
    }
    if next != boundaries.len() || cells != native_width {
        return Err(ProjectionError::ContentProjection);
    }
    Ok(widths)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use mant_ir::{
        DisplayLabel, DisplayRole, DisplayRow, DisplayRun, DisplayStyle, DisplaySurface,
    };

    use super::*;

    fn one_run(text: &str, width: u32) -> FixedBody {
        let key = NonZeroU32::new(1).unwrap();
        FixedBody {
            root_configuration_hint: false,
            surface: DisplaySurface {
                text: text.to_owned(),
                rows: vec![DisplayRow {
                    key,
                    first_run: key,
                    run_count: 1,
                    column_count: width,
                    break_after: false,
                }],
                runs: vec![DisplayRun {
                    key,
                    row: key,
                    column: 0,
                    width,
                    byte_start: 0,
                    byte_count: text.len() as u64,
                    label: DisplayLabel {
                        owner: None,
                        link: None,
                        source: None,
                        style: DisplayStyle {
                            bold: false,
                            underline: false,
                        },
                        role: DisplayRole::Body,
                    },
                }],
            },
            headings: Vec::new(),
            owners: Vec::new(),
            links: Vec::new(),
            anchors: Vec::new(),
            regions: Vec::new(),
        }
    }

    fn slice(start_byte: u64, end_byte: u64) -> OutputSlice {
        OutputSlice {
            run: NonZeroU32::new(1).unwrap(),
            start_byte,
            end_byte,
        }
    }

    #[test]
    fn fragmented_multibyte_run_uses_one_native_cell_mapping() {
        let fixed = one_run("a界b", 4);
        let slices = [slice(4, 5), slice(0, 1), slice(1, 4)];
        let view = copy_parts(&fixed, slices, 3, 64).unwrap();
        assert_eq!(
            view.parts
                .iter()
                .map(|part| (part.text.as_str(), part.column, part.width))
                .collect::<Vec<_>>(),
            [("a", 0, 1), ("界", 1, 2), ("b", 3, 1)]
        );
        assert_eq!(
            copy_parts(&fixed, slices, 2, 64),
            Err(ProjectionError::ContentProjection)
        );
        assert_eq!(
            copy_parts(&fixed, slices, 3, 4),
            Err(ProjectionError::ContentProjection)
        );
    }

    #[test]
    fn many_fragments_in_one_run_do_not_rescan_each_prefix() {
        let text = "x".repeat(8_192);
        let fixed = one_run(&text, 8_192);
        let slices = (0..8_192).map(|byte| slice(byte, byte + 1));
        let view = copy_parts(&fixed, slices, 8_192, 16_384).unwrap();
        assert_eq!(view.parts.len(), 8_192);
        assert_eq!(view.parts[8_191].column, 8_191);
    }
}
