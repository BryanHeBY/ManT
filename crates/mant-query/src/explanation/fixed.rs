//! Conservative explanation from surviving native Fixed owner evidence.
//!
//! No Flow block, inline tree, literal mention or declaration relationship is
//! synthesized from terminal geometry. A complete native definition head is
//! the only selectable form/name, and its copied fragments retain final runs.

use std::{collections::BTreeMap, num::NonZeroU32};

use mant_ir::{
    DOCUMENT_ROOT_ID, Document, FixedBody, FixedSectionReader, OutlinePath, OwnerMark,
    ResolvedContent, SemanticEntry, SemanticIndex, TextSelection,
};
use mant_protocol::{
    EvidenceBasis, EvidenceClass, EvidenceCounts, EvidenceOrder, ExplanationContent,
    ExplanationEntry, ExplanationEvidence, ExplanationFixedFormRange, ExplanationFixedPart,
    ExplanationFixedSelection, ExplanationFormMatch, ExplanationIdentityField,
    ExplanationNameBinding, ExplanationNameMatch, ExplanationOccurrence, ExplanationOutcome,
    ExplanationQuery, ExplanationSchema, ExplanationTruncation, OutlineNodeReference,
    OutlineReference, OutlineTrail, QueryExplanation,
};

use super::{ExplanationError, materialize::Budget};

struct IndexedOwner<'a> {
    entry: &'a SemanticEntry,
    path: OutlinePath,
}

/// A Fixed result only claims directly evidenced complete heads. Native body
/// slices are returned separately and never borrowed from a following owner.
pub(super) fn response(
    resolved: &ResolvedContent,
    document: &Document,
    fixed: &FixedBody,
    query: &ExplanationQuery,
) -> Result<(QueryExplanation, u32), ExplanationError> {
    mant_ir::validate_document_sources(document).map_err(|_| ExplanationError::InvalidFixed)?;
    let reader = FixedSectionReader::new(fixed).map_err(|_| ExplanationError::InvalidFixed)?;
    let index = SemanticIndex::build(document);
    let indexed = collect_indexed(&index, &reader);
    let requested = query.entry.trim();
    let mut candidates = Vec::new();
    let mut truncated = false;
    for owner in &fixed.owners {
        let Some(indexed) = indexed.get(&owner.key) else {
            continue;
        };
        let name = indexed.entry.names.iter().any(|value| value == requested);
        let form = indexed.entry.forms.iter().any(|value| value == requested);
        let id = indexed.entry.id.as_str() == requested;
        let path = indexed.path.to_string() == requested;
        if !(name || form || id || path) {
            continue;
        }
        if candidates.len() == mant_protocol::MAX_EXPLANATION_CANDIDATES {
            truncated = true;
            break;
        }
        candidates.push((owner, indexed, name, form, id, path));
    }
    let total = u32::try_from(candidates.len()).expect("bounded Fixed candidates");
    let mut budget = Budget(query.options.content_bytes as usize);
    let mut counts = EvidenceCounts::default();
    let mut evidence = Vec::new();
    for (ordinal, (owner, _indexed_owner, name, form, id, path)) in candidates.iter().enumerate() {
        let selected = ordinal >= query.options.offset as usize
            && evidence.len() < query.options.limit as usize;
        counts.record(EvidenceClass::DirectEntry, selected);
        if selected {
            evidence.push(materialize_owner(
                fixed,
                &reader,
                &indexed,
                owner,
                u32::try_from(ordinal).expect("bounded Fixed ordinal"),
                requested,
                (*name, *form, *id, *path),
                &mut budget,
            )?);
        }
    }
    let returned = u32::try_from(evidence.len()).expect("bounded Fixed page");
    let end = query.options.offset.saturating_add(returned);
    let used = query
        .options
        .content_bytes
        .saturating_sub(u32::try_from(budget.0).expect("bounded Fixed copy budget"));
    let content_omitted = evidence
        .iter()
        .any(ExplanationEvidence::has_omitted_content);
    Ok((
        QueryExplanation {
            supports: Vec::new(),
            content_projection: None,
            order: EvidenceOrder::ClassThenSource,
            counts,
            schema: ExplanationSchema::V0Dot12,
            query: ExplanationQuery {
                entry: requested.to_owned(),
                options: query.options,
            },
            label: resolved.label.clone(),
            address: resolved.address.clone(),
            producer: Some(mant_protocol::Producer::for_document(document)),
            source_context: Some(mant_protocol::SourceContext::from(document)),
            outcome: if total == 0 {
                ExplanationOutcome::NoEvidence
            } else {
                ExplanationOutcome::Evidence
            },
            total,
            returned,
            next_offset: (end < total).then_some(end),
            truncation: ExplanationTruncation {
                candidates: truncated,
                relations: false,
                content: content_omitted,
            },
            semantics_complete: crate::projection::semantics_complete(&document.diagnostics),
            diagnostics: document.diagnostics.clone(),
            evidence,
        },
        used,
    ))
}

fn collect_indexed<'a>(
    index: &'a SemanticIndex,
    reader: &FixedSectionReader<'_>,
) -> BTreeMap<NonZeroU32, IndexedOwner<'a>> {
    let mut map = BTreeMap::new();
    add_entries(index, index.root(), None, &[], &mut map);
    for heading in &reader.fixed().headings {
        let Some(OutlinePath::Section(coordinates)) = reader.path(heading.key) else {
            continue;
        };
        let section = coordinates
            .iter()
            .map(|part| part.get())
            .collect::<Vec<_>>();
        let source = section.iter().map(|part| part - 1).collect::<Vec<_>>();
        add_entries(
            index,
            index.section_at(&source),
            Some(&section),
            &[],
            &mut map,
        );
    }
    map
}

fn add_entries<'a>(
    index: &SemanticIndex,
    entries: &'a [SemanticEntry],
    section: Option<&[usize]>,
    prefix: &[usize],
    result: &mut BTreeMap<NonZeroU32, IndexedOwner<'a>>,
) {
    for (position, entry) in entries.iter().enumerate() {
        let mut indices = prefix.to_vec();
        indices.push(position + 1);
        let Some(path) = OutlinePath::nested_entry(section, &indices) else {
            continue;
        };
        if let Some(mant_ir::ContentReveal::FixedOwner { key }) = index.owner_at(&path) {
            result.insert(*key, IndexedOwner { entry, path });
        }
        add_entries(index, &entry.children, section, &indices, result);
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // One owner atomically closes its evidence and content budget.
fn materialize_owner(
    fixed: &FixedBody,
    reader: &FixedSectionReader<'_>,
    indexed: &BTreeMap<NonZeroU32, IndexedOwner<'_>>,
    owner: &OwnerMark,
    ordinal: u32,
    requested: &str,
    matched: (bool, bool, bool, bool),
    budget: &mut Budget,
) -> Result<ExplanationEvidence, ExplanationError> {
    let selected = indexed
        .get(&owner.key)
        .ok_or(ExplanationError::InvalidFixed)?;
    let (name_match, form_match, id_match, path_match) = matched;
    let outline = trail(reader, indexed, owner, selected)?;
    let [expected] = selected.entry.forms.as_slice() else {
        return Err(ExplanationError::InvalidFixed);
    };
    // Match facts are self-contained even when the optional owner excerpt
    // cannot fit. Charge them before entry metadata and its display bindings.
    let mut evidence_bases = Vec::new();
    let mut match_details_omitted = false;
    if name_match {
        let basis = EvidenceBasis::Name {
            matches: vec![ExplanationNameMatch {
                name: requested.to_owned(),
                occurrences: Vec::new(),
            }],
        };
        if budget.take(&basis) {
            evidence_bases.push(basis);
        } else {
            match_details_omitted = true;
        }
    }
    if form_match {
        let basis = EvidenceBasis::Form {
            matches: vec![ExplanationFormMatch {
                source_form_index: 0,
                text: requested.to_owned(),
                occurrences: Vec::new(),
            }],
        };
        if budget.take(&basis) {
            evidence_bases.push(basis);
        } else {
            match_details_omitted = true;
        }
    }
    let form = selection(fixed, &owner.head, budget.0);
    if form
        .as_ref()
        .is_some_and(|form| form.complete_text().as_deref() != Some(expected.as_str()))
    {
        return Err(ExplanationError::InvalidFixed);
    }
    let retained_entry = form.and_then(|form| {
        let occurrence = ExplanationOccurrence {
            source_occurrence_index: 0,
            forms: Vec::new(),
            fixed_forms: vec![ExplanationFixedFormRange {
                form_index: 0,
                start_scalar: 0,
                end_scalar: expected.chars().count() as u64,
            }],
            content: Vec::new(),
        };
        let entry = ExplanationEntry {
            kind: selected.entry.kind,
            case: selected.entry.case,
            names: selected.entry.names.clone(),
            forms: Vec::new(),
            fixed_forms: vec![form],
            name_bindings: vec![ExplanationNameBinding {
                name_index: 0,
                occurrences: vec![occurrence],
            }],
            alias_groups: Vec::new(),
            alias_of: None,
            value_domain: None,
        };
        budget.take(&entry).then_some(entry)
    });
    let details_omitted = retained_entry.is_none();
    let occurrence = retained_entry
        .as_ref()
        .and_then(|entry| entry.name_bindings.first())
        .and_then(|binding| binding.occurrences.first())
        .cloned();
    for basis in &mut evidence_bases {
        let Some(occurrence) = &occurrence else {
            match_details_omitted = true;
            continue;
        };
        let mut with_position = basis.clone();
        match &mut with_position {
            EvidenceBasis::Name { matches } => matches[0].occurrences.push(occurrence.clone()),
            EvidenceBasis::Form { matches } => matches[0].occurrences.push(occurrence.clone()),
            _ => unreachable!("only charged matches precede identity"),
        }
        if budget.take_growth(basis, &with_position) {
            *basis = with_position;
        } else {
            match_details_omitted = true;
        }
    }
    if id_match || path_match {
        let mut fields = Vec::new();
        if id_match {
            fields.push(ExplanationIdentityField::Id);
        }
        if path_match {
            fields.push(ExplanationIdentityField::Path);
        }
        evidence_bases.push(EvidenceBasis::Identity { fields });
    }
    let body_parts = reader
        .owner_body_parts(owner.key)
        .ok_or(ExplanationError::InvalidFixed)?;
    let body_selection = if body_parts.iter().map(|part| part.slice).eq(owner
        .direct_body
        .parts
        .iter()
        .copied())
    {
        owner.direct_body.clone()
    } else {
        // A union of direct, nested-owner and transparent-region selections
        // has no single native logical-join chain. Do not invent searchable
        // continuity: the presentation reads physical rows, not these joins.
        TextSelection {
            parts: body_parts.iter().map(|part| part.slice).collect(),
            joins: vec![mant_ir::TextJoin::HardBoundary; body_parts.len().saturating_sub(1)],
        }
    };
    let content = selection(fixed, &body_selection, budget.0).and_then(|body| {
        let content = ExplanationContent::FixedOwner {
            key: owner.key,
            reading_body: body,
        };
        budget.take(&content).then_some(content)
    });
    let content_omitted = content.is_none();
    Ok(ExplanationEvidence {
        support: None,
        support_omitted: false,
        class: EvidenceClass::DirectEntry,
        ordinal,
        outline,
        block_path: None,
        source: owner.source,
        bases: evidence_bases,
        previews: Vec::new(),
        previews_omitted: false,
        entry: retained_entry,
        content,
        details_omitted,
        match_details_omitted: match_details_omitted
            || details_omitted && (name_match || form_match),
        name_bindings_omitted: false,
        content_omitted,
    })
}

fn selection(
    fixed: &FixedBody,
    source: &TextSelection,
    maximum_bytes: usize,
) -> Option<ExplanationFixedSelection> {
    if source.parts.len() > mant_protocol::MAX_EXPLANATION_POSITIONS
        || source.parts.iter().try_fold(0usize, |total, part| {
            total.checked_add(usize::try_from(part.end_byte.checked_sub(part.start_byte)?).ok()?)
        })? > maximum_bytes
    {
        return None;
    }
    let mut parts = Vec::with_capacity(source.parts.len());
    for slice in &source.parts {
        let run = fixed
            .surface
            .runs
            .get(usize::try_from(slice.run.get() - 1).ok()?)?;
        let text = fixed.surface.run_text(slice.run)?;
        let start = usize::try_from(slice.start_byte).ok()?;
        let end = usize::try_from(slice.end_byte).ok()?;
        let (column, width) = if start == 0 && end == text.len() {
            (run.column, run.width)
        } else {
            // A clipped run needs a checked UTF-8-scalar to terminal-cell
            // mapping. Never treat byte offsets as cell offsets or guess when
            // the native run width disagrees with this mapping.
            use unicode_width::UnicodeWidthStr;
            if u32::try_from(text.width()).ok()? != run.width {
                return None;
            }
            (
                run.column
                    .checked_add(u32::try_from(text.get(..start)?.width()).ok()?)?,
                u32::try_from(text.get(start..end)?.width()).ok()?,
            )
        };
        parts.push(ExplanationFixedPart {
            slice: *slice,
            row: run.row,
            run_column: run.column,
            column,
            width,
            style: run.label.style,
            text: text.get(start..end)?.to_owned(),
            source: run.label.source,
        });
    }
    let selection = ExplanationFixedSelection {
        parts,
        joins: source.joins.clone(),
    };
    selection.validate().ok()?;
    Some(selection)
}

fn trail(
    reader: &FixedSectionReader<'_>,
    indexed: &BTreeMap<NonZeroU32, IndexedOwner<'_>>,
    owner: &OwnerMark,
    selected: &IndexedOwner<'_>,
) -> Result<OutlineTrail, ExplanationError> {
    let mut ancestors = Vec::new();
    ancestors.push(OutlineReference {
        path: OutlinePath::DocumentRoot.to_string().into(),
        id: DOCUMENT_ROOT_ID.into(),
        title: crate::selectors::DOCUMENT_ROOT_TITLE.to_owned(),
    });
    if let Some(section) = owner.section {
        for heading in reader
            .breadcrumbs(section)
            .ok_or(ExplanationError::InvalidFixed)?
        {
            ancestors.push(OutlineReference {
                path: reader
                    .path(heading.key)
                    .ok_or(ExplanationError::InvalidFixed)?
                    .to_string()
                    .into(),
                id: heading.id.clone(),
                title: reader
                    .label(heading.key)
                    .ok_or(ExplanationError::InvalidFixed)?,
            });
        }
    }
    let mut parents = Vec::new();
    let mut parent = owner.parent;
    while let Some(key) = parent {
        let mark = reader
            .fixed()
            .owners
            .get(usize::try_from(key.get() - 1).map_err(|_| ExplanationError::InvalidFixed)?)
            .ok_or(ExplanationError::InvalidFixed)?;
        if let Some(entry) = indexed.get(&key) {
            parents.push(OutlineReference {
                path: entry.path.to_string().into(),
                id: entry.entry.id.clone(),
                title: entry.entry.forms.first().cloned().unwrap_or_default(),
            });
        }
        parent = mark.parent;
    }
    ancestors.extend(parents.into_iter().rev());
    Ok(OutlineTrail {
        ancestors,
        node: OutlineNodeReference::DocumentEntry {
            path: selected.path.to_string().into(),
            id: selected.entry.id.clone(),
            title: selected.entry.forms.first().cloned().unwrap_or_default(),
            entry_kind: selected.entry.kind,
            case: selected.entry.case,
            names: selected.entry.names.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use mant_ir::{
        DisplayLabel, DisplayPoint, DisplayRole, DisplayRow, DisplayRun, DisplayStyle,
        DisplaySurface, DocumentBody, DocumentMeta, HeadingMark, LinkMark, LinkTarget, NodeId,
        OutputSlice, OwnerRole, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey,
        SourceRecord, TextJoin,
    };
    use mant_protocol::{ContentSelector, EntryProjection, OutlineNode};

    use super::*;

    fn key(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("fixture key is positive")
    }

    fn slices(keys: &[u32], joins: Vec<TextJoin>, lengths: &[u64]) -> TextSelection {
        TextSelection {
            parts: keys
                .iter()
                .map(|&run| OutputSlice {
                    run: key(run),
                    start_byte: 0,
                    end_byte: lengths[(run - 1) as usize],
                })
                .collect(),
            joins,
        }
    }

    #[allow(clippy::too_many_lines)] // One complete Fixed snapshot exposes every tested owner/section edge.
    fn fixture() -> ResolvedContent {
        // This is a synthetic, already-final Fixed snapshot: it tests IR and
        // projection contracts only, not a claim about roff line wrapping.
        let texts = [
            "PREFACE",
            "OPTIONS",
            "printf",
            "(3)",
            "first body",
            "empty body",
            "NESTED",
            "child text",
        ];
        let lengths = texts.map(|text| text.len() as u64);
        let mut arena = String::new();
        let mut rows = Vec::new();
        let mut runs = Vec::new();
        for (index, text) in texts.iter().enumerate() {
            let number = u32::try_from(index + 1).unwrap();
            let owner = match number {
                3..=5 => Some(key(1)),
                6 => Some(key(2)),
                _ => None,
            };
            let link = (3..=4).contains(&number).then(|| key(1));
            rows.push(DisplayRow {
                key: key(number),
                first_run: key(number),
                run_count: 1,
                column_count: u32::try_from(text.len()).unwrap(),
                break_after: index + 1 < texts.len(),
            });
            runs.push(DisplayRun {
                key: key(number),
                row: key(number),
                column: 0,
                width: u32::try_from(text.len()).unwrap(),
                byte_start: u64::try_from(arena.len()).unwrap(),
                byte_count: lengths[index],
                label: DisplayLabel {
                    owner,
                    link,
                    source: Some(SourceKey::FIRST),
                    style: DisplayStyle {
                        bold: false,
                        underline: false,
                    },
                    role: DisplayRole::Body,
                },
            });
            arena.push_str(text);
        }
        let hard = || TextJoin::HardBoundary;
        let mut fixed = FixedBody {
            surface: DisplaySurface {
                text: arena,
                rows,
                runs,
            },
            headings: vec![
                HeadingMark {
                    key: key(1),
                    id: NodeId::from("options"),
                    fragment_aliases: Vec::new(),
                    generated_fragment_aliases: Vec::new(),
                    rendered_fragment_aliases: Vec::new(),
                    parent: None,
                    level_hint: 1,
                    at: DisplayPoint::RunBoundary {
                        run: key(2),
                        byte: 0,
                    },
                    title: slices(&[2], Vec::new(), &lengths),
                    direct_body: slices(&[3, 4, 5, 6], vec![hard(), hard(), hard()], &lengths),
                    source: None,
                },
                HeadingMark {
                    key: key(2),
                    id: NodeId::from("nested"),
                    fragment_aliases: Vec::new(),
                    generated_fragment_aliases: Vec::new(),
                    rendered_fragment_aliases: Vec::new(),
                    parent: Some(key(1)),
                    level_hint: 2,
                    at: DisplayPoint::RunBoundary {
                        run: key(7),
                        byte: 0,
                    },
                    title: slices(&[7], Vec::new(), &lengths),
                    direct_body: slices(&[8], Vec::new(), &lengths),
                    source: None,
                },
            ],
            owners: vec![
                OwnerMark {
                    key: key(1),
                    id: NodeId::from("owner-printf"),
                    parent: None,
                    section: Some(key(1)),
                    role: OwnerRole::Definition,
                    entry: None,
                    head: slices(&[3, 4], vec![TextJoin::DirectContact], &lengths),
                    direct_body: slices(&[5], Vec::new(), &lengths),
                    empty_point: None,
                    source: None,
                },
                OwnerMark {
                    key: key(2),
                    id: NodeId::from("owner-empty"),
                    parent: None,
                    section: Some(key(1)),
                    role: OwnerRole::Definition,
                    entry: None,
                    head: TextSelection {
                        parts: Vec::new(),
                        joins: Vec::new(),
                    },
                    direct_body: slices(&[6], Vec::new(), &lengths),
                    empty_point: None,
                    source: None,
                },
            ],
            links: vec![LinkMark {
                key: key(1),
                target: Some(LinkTarget::External {
                    uri: "https://example.test/printf".into(),
                }),
                label: slices(&[3, 4], vec![TextJoin::DirectContact], &lengths),
                source: None,
            }],
            anchors: Vec::new(),
            regions: Vec::new(),
        };
        let form = fixed.owners[0].head.clone();
        fixed.owners[0].entry = Some(mant_ir::EntryFacts {
            name_bindings: vec![mant_ir::EntryNameBinding {
                name: 0,
                occurrences: vec![form.clone()],
                evidence: mant_ir::EntryNameEvidence::Lexical,
            }],
            alias_groups: Vec::new(),
            alias_of: None,
            forms: vec![form],
            id: fixed.owners[0].id.clone(),
            kind: mant_ir::EntryKind::Term,
            case: mant_ir::NameCase::Sensitive,
            names: vec!["printf(3)".into()],
            value_domain: None,
        });
        fixed.validate().expect("self-contained Fixed fixture");
        ResolvedContent {
            label: "Fixed fixture".into(),
            address: None,
            document: Some(Document {
                parser: None,
                sources: vec![SourceRecord {
                    key: SourceKey::FIRST,
                    identity: SourceIdentity::Anonymous {
                        name: "synthetic-fixed".into(),
                    },
                    format: SourceFormat::Man,
                    decoded_byte_length: 0,
                    content_sha256: None,
                    coordinates: SourceCoordinates::NativeNormalizedBytes,
                }],
                root_source: SourceKey::FIRST,
                body: DocumentBody::Fixed(fixed),
                meta: DocumentMeta::default(),
                fragment_aliases: Vec::new(),
                diagnostics: Vec::new(),
            }),
            tldr: None,
        }
    }

    #[test]
    fn complete_linked_cross_row_head_is_the_only_semantic_entry() {
        let resolved = fixture();
        let document = resolved.document.as_ref().unwrap();
        let index = SemanticIndex::build(document);
        let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
            unreachable!()
        };
        assert_eq!(fixed.links[0].label, fixed.owners[0].head);
        assert!(index.root().is_empty());
        assert_eq!(index.section("options").len(), 1);
        assert_eq!(index.section("options")[0].forms, ["printf(3)"]);
        assert_eq!(index.section("options")[0].names, ["printf(3)"]);
        assert!(index.section("nested").is_empty());

        let outline =
            crate::projection::build_outline_projection(&resolved, EntryProjection::All, None)
                .expect("Fixed semantic outline");
        assert_eq!(outline.nodes.len(), 2); // root preface, then OPTIONS
        assert_eq!(outline.nodes[0].path(), "root");
        let section = &outline.nodes[1];
        assert_eq!(section.path(), "1");
        assert_eq!(section.children().len(), 2); // entry, then nested section
        assert!(matches!(
            &section.children()[0],
            OutlineNode::DocumentEntry { forms, owner, .. }
                if forms.len() == 1 && forms[0] == "printf(3)" && matches!(owner.as_ref(), mant_ir::ContentReveal::FixedOwner { key: owner_key } if *owner_key == key(1))
        ));
        assert_eq!(section.children()[1].path(), "1.1");

        let selected = crate::projection::build_outline_projection(
            &resolved,
            EntryProjection::All,
            Some(ContentSelector::path("1.1")),
        )
        .expect("nested native section path");
        assert_eq!(selected.nodes[0].path(), "1.1");
    }

    #[test]
    fn explain_keeps_linked_form_coordinates_and_owner_local_body() {
        let resolved = fixture();
        let result = super::super::select_explanation(&resolved, "printf(3)").unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.counts.direct_entry.total, 1);
        assert_eq!(result.evidence[0].outline.path(), "1/e1");
        let record = &result.evidence[0];
        let entry = record.entry.as_ref().expect("bounded Fixed form details");
        assert_eq!(
            entry.fixed_forms[0].complete_text().as_deref(),
            Some("printf(3)")
        );
        assert_eq!(
            entry.fixed_forms[0]
                .parts
                .iter()
                .map(|part| part.row.get())
                .collect::<Vec<_>>(),
            vec![3, 4]
        );
        let occurrence = match &record.bases[0] {
            EvidenceBasis::Name { matches } => &matches[0].occurrences[0],
            other => panic!("expected native name evidence, got {other:?}"),
        };
        assert_eq!(
            occurrence.fixed_forms[0]
                .resolve(&entry.fixed_forms)
                .as_deref(),
            Some("printf(3)")
        );
        assert!(!record.name_bindings_omitted);
        assert_eq!(entry.name_bindings[0].name_index, 0);
        assert_eq!(
            entry.name_bindings[0].occurrences.as_slice(),
            std::slice::from_ref(occurrence)
        );
        let Some(ExplanationContent::FixedOwner {
            key: owner_key,
            reading_body,
        }) = &record.content
        else {
            panic!("Fixed owner body must not become a Flow block");
        };
        assert_eq!(*owner_key, key(1));
        assert_eq!(reading_body.complete_text().as_deref(), Some("first body"));
        assert!(
            !reading_body
                .parts
                .iter()
                .any(|part| part.text == "empty body")
        );
        result.validate_references().unwrap();
        serde_json::from_value::<QueryExplanation>(serde_json::to_value(&result).unwrap()).unwrap();

        assert_eq!(
            super::super::select_explanation(&resolved, "empty body")
                .unwrap()
                .total,
            0
        );
        assert_eq!(
            super::super::select_explanation(&resolved, "owner-empty")
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn fixed_match_facts_precede_optional_entry_and_binding_copies() {
        let resolved = fixture();
        let document = resolved.document.as_ref().unwrap();
        let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
            unreachable!()
        };
        let basis = EvidenceBasis::Name {
            matches: vec![ExplanationNameMatch {
                name: "printf(3)".into(),
                occurrences: Vec::new(),
            }],
        };
        let query = ExplanationQuery {
            entry: "printf(3)".into(),
            options: mant_protocol::ExplanationOptions {
                content_bytes: u32::try_from(serde_json::to_vec(&basis).unwrap().len()).unwrap(),
                ..Default::default()
            },
        };
        let (result, _) = super::response(&resolved, document, fixed, &query).unwrap();
        let record = &result.evidence[0];
        assert!(matches!(
            record.bases.as_slice(),
            [EvidenceBasis::Name { .. }]
        ));
        assert!(record.entry.is_none());
        assert!(record.details_omitted);
        assert!(record.match_details_omitted);
        result.validate_references().unwrap();
    }

    #[test]
    fn section_reader_keeps_direct_subtree_and_root_preface_distinct() {
        let resolved = fixture();
        let document = resolved.document.as_ref().unwrap();
        let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
            unreachable!()
        };
        let reader = FixedSectionReader::new(fixed).unwrap();
        let preface = reader.root_preface_parts().unwrap();
        assert_eq!(
            preface.iter().map(|part| part.text).collect::<Vec<_>>(),
            ["PREFACE"]
        );
        let direct = reader.direct_parts(key(1)).unwrap();
        assert_eq!(
            direct.iter().map(|part| part.text).collect::<Vec<_>>(),
            ["OPTIONS", "printf", "(3)", "first body", "empty body"]
        );
        let subtree = reader.subtree_parts(key(1)).unwrap();
        assert_eq!(
            subtree.iter().map(|part| part.text).collect::<Vec<_>>(),
            [
                "OPTIONS",
                "printf",
                "(3)",
                "first body",
                "empty body",
                "NESTED",
                "child text"
            ]
        );
    }
}
