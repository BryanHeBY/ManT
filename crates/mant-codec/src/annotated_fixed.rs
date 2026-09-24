//! Direct projection from the checked native post-device result to Fixed IR.
//!
//! Native owns the only display byte arena. This module changes key domains
//! and closes typed references; it never replays roff or builds Flow prose.

use std::{fmt, num::NonZeroU32};

use libmandoc_rs::annotated::{
    AnnotatedDisplayPoint, AnnotatedDocument, AnnotatedError, AnnotatedMark, AnnotatedRenderer,
    AnnotatedTextJoin, AnnotationCheckState, AnnotationDimension, AnnotationIssueReason,
    AnnotationScope,
};
use libmandoc_rs::{InputFormat, SourceBundle};
use mant_ir::{
    AnchorMark, CoverageScope, Diagnostic, DiagnosticImpact, DiagnosticLevel, DisplayLabel,
    DisplayPoint, DisplayRole, DisplayRow, DisplayRun, DisplayStyle, DisplaySurface, Document,
    DocumentBody, DocumentMeta, EntryFacts, EntryKind, EntryNameBinding, EntryNameEvidence,
    FixedBody, FragmentAlias, HeadingMark, LinkMark, LinkTarget, NameCase, NodeId, OutputSlice,
    OwnerHeadComponent, OwnerHeadRole, OwnerMark, OwnerRole, ParameterKind, ParserInfo, RegionKind,
    RegionMark, SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord,
    SourceSpan, TextJoin, TextSelection, validate_document, validate_document_sources,
};

/// A native render failure or a relation that cannot be represented honestly
/// in the current Fixed IR contract.
#[derive(Debug)]
pub enum AnnotatedProjectionError {
    /// The native renderer did not return a complete checked result.
    Native(AnnotatedError),
    /// An owned result has no lossless mapping into current typed IR.
    Relation(&'static str),
    /// An owned result failed a detailed typed relationship check.
    RelationDetail(String),
}

impl fmt::Display for AnnotatedProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(error) => error.fmt(formatter),
            Self::Relation(message) => formatter.write_str(message),
            Self::RelationDetail(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for AnnotatedProjectionError {}

impl From<AnnotatedError> for AnnotatedProjectionError {
    fn from(error: AnnotatedError) -> Self {
        Self::Native(error)
    }
}

type Result<T> = std::result::Result<T, AnnotatedProjectionError>;

/// Render one authorized bundle and project its checked final device output.
///
/// # Errors
/// Returns the native failure or a typed relationship that cannot be closed.
pub fn project_annotated_manual(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
) -> Result<Document> {
    lower_annotated_document(AnnotatedRenderer::default().render_bundle(root, bundle, format)?)
}

/// Consume an owned native display result as the one Fixed document body.
///
/// # Errors
/// Returns a relation error rather than synthesizing missing display, source,
/// occurrence or navigation evidence.
#[allow(clippy::too_many_lines)] // One checked native transfer and typed-document assembly.
pub fn lower_annotated_document(mut page: AnnotatedDocument) -> Result<Document> {
    let identities = Identities::new(&page)?;
    let keys = KeyMap::new(&page, &identities)?;
    let sources = sources(&page)?;
    let mut row_keys = vec![0_u32; page.runs.len()];
    let rows = rows(&page, &mut row_keys)?;
    let mut runs = Vec::with_capacity(page.runs.len());
    for (index, run) in page.runs.iter().enumerate() {
        let row = key(row_keys[index], "native run has no final row")?;
        runs.push(DisplayRun {
            key: key(run.key, "native run has zero key")?,
            row,
            column: run.column,
            width: run.width,
            byte_start: run.byte_start,
            byte_count: run.byte_count,
            label: DisplayLabel {
                owner: keys.nearest(run.label.owner, 2)?,
                link: keys.lookup(run.label.link, 3)?,
                source: source_key(run.label.source)?,
                style: DisplayStyle {
                    bold: run.label.style & 1 != 0,
                    underline: run.label.style & 8 != 0,
                },
                role: match run.label.role {
                    1 => DisplayRole::Body,
                    4 => DisplayRole::DirectDraw,
                    5 => DisplayRole::Layout,
                    _ => return Err(AnnotatedProjectionError::Relation("unknown display role")),
                },
            },
        });
    }
    let mut headings = Vec::new();
    let mut owners = Vec::new();
    let mut links = Vec::new();
    let mut resolution_diagnostics = Vec::new();
    let mut anchors = Vec::new();
    let mut regions = Vec::new();
    // mdoc_macro.c::phrase_ta() creates a distinct BODY for each .It
    // column.  The native body's singular pointer is only its latest BODY;
    // index every direct BODY once before projecting owners.
    let mut owner_bodies = vec![Vec::new(); page.marks.len() + 1];
    let mut owner_components = vec![Vec::new(); page.marks.len() + 1];
    for mark in &page.marks {
        if mark.kind == 5 && mark.region_kind == 4 {
            let owner = page
                .marks
                .get(mark.parent.saturating_sub(1) as usize)
                .ok_or(AnnotatedProjectionError::Relation(
                    "owner body has no parent",
                ))?;
            if owner.key != mark.parent || owner.kind != 2 {
                return Err(AnnotatedProjectionError::Relation(
                    "owner body parent is not an owner",
                ));
            }
            owner_bodies[mark.parent as usize].push(mark.key);
        } else if mark.kind == 6 {
            let head = page
                .marks
                .get(mark.parent.saturating_sub(1) as usize)
                .ok_or(AnnotatedProjectionError::Relation(
                    "head component has no region",
                ))?;
            let owner = page
                .marks
                .get(head.parent.saturating_sub(1) as usize)
                .ok_or(AnnotatedProjectionError::Relation(
                    "head component has no owner",
                ))?;
            if head.key != mark.parent
                || head.kind != 5
                || head.region_kind != 3
                || owner.kind != 2
                || owner.title_region != head.key
                || mark.owner != head.key
            {
                return Err(AnnotatedProjectionError::Relation(
                    "head component escapes its owner head",
                ));
            }
            owner_components[owner.key as usize].push(mark.key);
        }
    }
    for mark in &page.marks {
        match mark.kind {
            1 => headings.push(project_heading(&page, &keys, &identities, mark)?),
            2 => owners.push(project_owner(
                &page,
                &keys,
                &identities,
                mark,
                &owner_bodies[mark.key as usize],
                &owner_components[mark.key as usize],
            )?),
            3 => links.push(project_link(
                &page,
                &keys,
                &identities,
                mark,
                &mut resolution_diagnostics,
            )?),
            4 if !identities.consumed_anchor(mark.key)? => {
                anchors.push(project_anchor(&keys, &identities, mark)?);
            }
            4 | 6 => {}
            5 => regions.push(project_region(&page, &keys, mark)?),
            _ => {
                return Err(AnnotatedProjectionError::Relation(
                    "unknown native mark kind",
                ));
            }
        }
    }
    // All borrowed native selections and marks have been projected. Transfer
    // the sole display arena into Fixed without cloning the page body.
    let surface = DisplaySurface {
        text: std::mem::take(&mut page.text),
        rows,
        runs,
    };
    let mut fixed = FixedBody {
        surface,
        headings,
        owners,
        links,
        anchors,
        regions,
    };
    for region in &fixed.regions {
        if let Some(candidate) = region.continuation_of {
            let owner = fixed.owners.get_mut((candidate.get() - 1) as usize).ok_or(
                AnnotatedProjectionError::Relation("continuation candidate is missing"),
            )?;
            if owner.hanging_continuation.replace(region.key).is_some() {
                return Err(AnnotatedProjectionError::Relation(
                    "candidate has multiple hanging continuations",
                ));
            }
        }
    }
    // An RS containing only a nested TP has no direct glyph selection, but
    // the native OwnerHead still proves a structural description. Keep one
    // direct child key as an O(1) read-time witness; a tbl region alone does
    // not authorize a semantic hanging entry.
    for region in &fixed.regions {
        if region.kind != RegionKind::OwnerHead || region.owner.is_none() {
            continue;
        }
        let Some(continuation) = region
            .parent
            .and_then(|key| fixed.regions.get((key.get() - 1) as usize))
            .filter(|parent| parent.kind == RegionKind::HangingContinuation)
        else {
            continue;
        };
        let Some(candidate) = continuation.continuation_of else {
            continue;
        };
        let owner = fixed.owners.get_mut((candidate.get() - 1) as usize).ok_or(
            AnnotatedProjectionError::Relation("nested continuation candidate is missing"),
        )?;
        owner.hanging_nested_head.get_or_insert(region.key);
    }
    // The checked native head is a borrowed display selection, not a
    // reconstructed Flow term. Keep its initial conservative identity in the
    // document so serialization and index rebuilding cannot diverge.
    let entries = fixed
        .owners
        .iter()
        .map(|owner| {
            if owner.hanging_candidate && !fixed.hanging_declaration_ready(owner) {
                return None;
            }
            if let Some(components) = fixed.lexical_component_names(owner) {
                let (names, name_bindings) = group_bindings(
                    components
                        .into_iter()
                        .map(|(name, selection, _)| (name, selection)),
                    EntryNameEvidence::Lexical,
                );
                return Some(EntryFacts {
                    name_bindings,
                    alias_groups: Vec::new(),
                    alias_of: None,
                    forms: vec![owner.head.clone()],
                    id: owner.id.clone(),
                    kind: EntryKind::Parameter {
                        parameter_kind: ParameterKind::Option,
                    },
                    case: NameCase::Sensitive,
                    names,
                    value_domain: None,
                });
            }
            if let Some(forms) = fixed.option_component_forms(owner) {
                let names = forms.iter().map(|(name, _)| name.clone()).collect();
                let (head_forms, name_bindings) = forms
                    .into_iter()
                    .enumerate()
                    .map(|(index, (_, selection))| {
                        (
                            selection.clone(),
                            EntryNameBinding {
                                name: index,
                                occurrences: vec![selection],
                                evidence: EntryNameEvidence::NativeMarkup,
                            },
                        )
                    })
                    .unzip();
                return Some(EntryFacts {
                    name_bindings,
                    alias_groups: Vec::new(),
                    alias_of: None,
                    forms: head_forms,
                    id: owner.id.clone(),
                    kind: EntryKind::Parameter {
                        parameter_kind: ParameterKind::Option,
                    },
                    case: NameCase::Sensitive,
                    names,
                    value_domain: None,
                });
            }
            if let Some(components) = fixed.option_component_names(owner) {
                let (names, name_bindings) = group_bindings(
                    components
                        .into_iter()
                        .map(|(name, selection, _)| (name, selection)),
                    EntryNameEvidence::NativeMarkup,
                );
                return Some(EntryFacts {
                    name_bindings,
                    alias_groups: Vec::new(),
                    alias_of: None,
                    forms: vec![owner.head.clone()],
                    id: owner.id.clone(),
                    kind: EntryKind::Parameter {
                        parameter_kind: ParameterKind::Option,
                    },
                    case: NameCase::Sensitive,
                    names,
                    value_domain: None,
                });
            }
            let Some(form) = fixed.owner_complete_form(owner) else {
                // Keep the complete physical HEAD in owner.head. Only this
                // authored, delimited Ic/Cm component is a logical form:
                // later Xo joins are Unknown and cannot be guessed into one.
                let (name, component) = fixed.literal_command_component(owner)?;
                let selection = component.clone();
                return Some(EntryFacts {
                    name_bindings: vec![EntryNameBinding {
                        name: 0,
                        occurrences: vec![selection.clone()],
                        evidence: EntryNameEvidence::NativeMarkup,
                    }],
                    alias_groups: Vec::new(),
                    alias_of: None,
                    forms: vec![selection],
                    id: owner.id.clone(),
                    kind: EntryKind::Command,
                    case: NameCase::Sensitive,
                    names: vec![name],
                    value_domain: None,
                });
            };
            // man_macro.c::blk_imp keeps the original man HEAD intact, and
            // man_term.c::pre_TP/pre_IP select its visible label. A complete
            // form may also contain arguments, so bind only the shared
            // grammar's exact name ranges to final-display sub-selections.
            if owner.head_role == Some(OwnerHeadRole::Lexical) && owner.head_role_prefix.is_none() {
                let found = mant_ir::literal_option_names(&form);
                let mut occurrences = Vec::with_capacity(found.len());
                for (name, range) in found {
                    let occurrence = fixed.selection_subrange(&owner.head, range)?;
                    if fixed.selection_text(&occurrence).as_deref() != Some(name.as_str()) {
                        return None;
                    }
                    occurrences.push((name, occurrence));
                }
                let (names, name_bindings) =
                    group_bindings(occurrences, EntryNameEvidence::Lexical);
                if !names.is_empty() {
                    return Some(EntryFacts {
                        name_bindings,
                        alias_groups: Vec::new(),
                        alias_of: None,
                        forms: vec![owner.head.clone()],
                        id: owner.id.clone(),
                        kind: EntryKind::Parameter {
                            parameter_kind: ParameterKind::Option,
                        },
                        case: NameCase::Sensitive,
                        names,
                        value_domain: None,
                    });
                }
            }
            let identity = native_head_identity(&fixed, owner, &form);
            // The parser-alive .IP hint is intentionally a broad candidate:
            // a styled prefix that fails the shared spelling or final-glyph
            // check is not a generic Term. Keep its display owner without
            // inventing a different semantic declaration.
            if owner.head_role == Some(OwnerHeadRole::Lexical)
                && owner.head_role_prefix.is_some()
                && identity.is_none()
            {
                return None;
            }
            let (kind, evidence, name, occurrence) = identity.unwrap_or_else(|| {
                (
                    EntryKind::Term,
                    EntryNameEvidence::Lexical,
                    form.clone(),
                    owner.head.clone(),
                )
            });
            let named = kind != EntryKind::Term || owner.head_role.is_some();
            Some(EntryFacts {
                name_bindings: named
                    .then_some(EntryNameBinding {
                        name: 0,
                        occurrences: vec![occurrence],
                        evidence,
                    })
                    .into_iter()
                    .collect(),
                alias_groups: Vec::new(),
                alias_of: None,
                forms: vec![owner.head.clone()],
                id: owner.id.clone(),
                kind,
                case: NameCase::Sensitive,
                names: named.then_some(name).into_iter().collect(),
                value_domain: None,
            })
        })
        .collect::<Vec<_>>();
    for (owner, entry) in fixed.owners.iter_mut().zip(entries) {
        owner.entry = entry;
    }
    fixed.validate().map_err(|error| {
        AnnotatedProjectionError::RelationDetail(format!("invalid projected Fixed body: {error}"))
    })?;
    let mut document = Document {
        parser: Some(ParserInfo {
            name: "libmandoc-annotated".to_owned(),
            version: libmandoc_rs::LIBMANDOC_VERSION.to_owned(),
        }),
        sources,
        root_source: key_source(page.root_source)?,
        body: DocumentBody::Fixed(fixed),
        meta: metadata(&page),
        fragment_aliases: Vec::new(),
        diagnostics: native_diagnostics(&page)?,
    };
    document
        .diagnostics
        .extend(coverage_diagnostics(&page, &keys)?);
    document.diagnostics.extend(resolution_diagnostics);
    validate_document_sources(&document)
        .map_err(|_| AnnotatedProjectionError::Relation("invalid projected source table"))?;
    if let Some(first) = validate_document(&document).into_iter().next() {
        return Err(AnnotatedProjectionError::RelationDetail(format!(
            "invalid projected document: {}: {}",
            first.code.as_deref().unwrap_or("unnamed"),
            first.message
        )));
    }
    Ok(document)
}

/// Stable name order with every distinct native occurrence retained.
fn group_bindings(
    found: impl IntoIterator<Item = (String, TextSelection)>,
    evidence: EntryNameEvidence,
) -> (Vec<String>, Vec<EntryNameBinding<TextSelection>>) {
    let mut indices: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut names = Vec::new();
    let mut bindings: Vec<EntryNameBinding<TextSelection>> = Vec::new();
    for (name, occurrence) in found {
        if let Some(&index) = indices.get(&name) {
            bindings[index].occurrences.push(occurrence);
        } else {
            let index = names.len();
            indices.insert(name.clone(), index);
            names.push(name);
            bindings.push(EntryNameBinding {
                name: index,
                occurrences: vec![occurrence],
                evidence,
            });
        }
    }
    (names, bindings)
}

fn native_head_identity(
    fixed: &FixedBody,
    owner: &OwnerMark,
    form: &str,
) -> Option<(EntryKind, EntryNameEvidence, String, TextSelection)> {
    // man_macro.c::blk_imp establishes a real TP/TQ head even without mdoc
    // markup. A complete surviving token can use the same source-neutral
    // option spelling rule; an argument suffix or incomplete join cannot.
    if owner.head_role == Some(OwnerHeadRole::Lexical)
        && owner.head_role_prefix.is_none()
        && mant_ir::lexical_option_token(form)
    {
        return Some((
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            EntryNameEvidence::Lexical,
            form.to_owned(),
            owner.head.clone(),
        ));
    }
    let leading = form.trim_start();
    let start = form.len() - leading.len();
    if owner.head_role == Some(OwnerHeadRole::Literal) {
        let (name, selection) = fixed.literal_command_component(owner)?;
        let end = start.checked_add(name.len())?;
        if form.get(start..end) != Some(name.as_str())
            || !form
                .get(end..)
                .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(char::is_whitespace))
            || fixed.selection_subrange(&owner.head, start..end).as_ref() != Some(selection)
        {
            return None;
        }
        return Some((
            EntryKind::Command,
            EntryNameEvidence::NativeMarkup,
            name,
            selection.clone(),
        ));
    }
    let role_prefix = owner.head_role_prefix.as_deref()?;
    if !leading.starts_with(role_prefix) {
        return None;
    }
    let (kind, name) = match owner.head_role? {
        OwnerHeadRole::Option => {
            crate::definitions::native_option_token(role_prefix).then(|| {
                (
                    EntryKind::Parameter {
                        parameter_kind: ParameterKind::Option,
                    },
                    role_prefix.to_owned(),
                )
            })?
        }
        OwnerHeadRole::Environment => (
            EntryKind::EnvironmentVariable,
            crate::definitions::environment_variable_alias(role_prefix)?,
        ),
        OwnerHeadRole::Lexical => mant_ir::lexical_option_token(role_prefix).then(|| {
            (
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                role_prefix.to_owned(),
            )
        })?,
        OwnerHeadRole::Literal => return None,
    };
    let end = start.checked_add(name.len())?;
    if owner.head_role == Some(OwnerHeadRole::Lexical)
        && !matches!(form.get(end..)?.chars().next(), None | Some('='))
        && !form.get(end..)?.starts_with(char::is_whitespace)
    {
        return None;
    }
    let occurrence = fixed.selection_subrange(&owner.head, start..end)?;
    (fixed.selection_text(&occurrence).as_deref() == Some(name.as_str())).then_some((
        kind,
        EntryNameEvidence::NativeMarkup,
        name,
        occurrence,
    ))
}

fn key(value: u32, error: &'static str) -> Result<NonZeroU32> {
    NonZeroU32::new(value).ok_or(AnnotatedProjectionError::Relation(error))
}

fn source_key(value: u32) -> Result<Option<SourceKey>> {
    if value == 0 {
        Ok(None)
    } else {
        Ok(Some(key_source(value)?))
    }
}

fn key_source(value: u32) -> Result<SourceKey> {
    SourceKey::new(value).ok_or(AnnotatedProjectionError::Relation("zero source key"))
}

fn sources(page: &AnnotatedDocument) -> Result<Vec<SourceRecord>> {
    page.sources
        .iter()
        .map(|source| {
            let name = source.logical_name.clone();
            Ok(SourceRecord {
                key: key_source(source.key)?,
                identity: match source.identity_kind {
                    1 => SourceIdentity::Path { name },
                    2 => SourceIdentity::BundleMember { name },
                    3 => SourceIdentity::Anonymous { name },
                    _ => {
                        return Err(AnnotatedProjectionError::Relation(
                            "unknown source identity",
                        ));
                    }
                },
                format: match source.format {
                    1 => SourceFormat::Man,
                    2 => SourceFormat::Mdoc,
                    _ => return Err(AnnotatedProjectionError::Relation("unknown source format")),
                },
                decoded_byte_length: source.decoded_length,
                content_sha256: source.hash,
                coordinates: match source.coordinate_kind {
                    1 => SourceCoordinates::DecodedUtf8Bytes,
                    2 => SourceCoordinates::NativeNormalizedBytes,
                    _ => {
                        return Err(AnnotatedProjectionError::Relation(
                            "unknown source coordinates",
                        ));
                    }
                },
            })
        })
        .collect()
}

fn rows(page: &AnnotatedDocument, row_keys: &mut [u32]) -> Result<Vec<DisplayRow>> {
    let mut rows = Vec::with_capacity(page.rows.len());
    for row in &page.rows {
        let first = usize::try_from(row.first_run)
            .map_err(|_| AnnotatedProjectionError::Relation("row start overflow"))?;
        let end = first
            .checked_add(row.run_count as usize)
            .ok_or(AnnotatedProjectionError::Relation("row run count overflow"))?;
        let owned = row_keys
            .get_mut(first..end)
            .ok_or(AnnotatedProjectionError::Relation(
                "row references missing runs",
            ))?;
        for item in owned {
            if *item != 0 {
                return Err(AnnotatedProjectionError::Relation(
                    "run belongs to two rows",
                ));
            }
            *item = row.key;
        }
        rows.push(DisplayRow {
            key: key(row.key, "zero row key")?,
            first_run: key(
                row.first_run
                    .checked_add(1)
                    .ok_or(AnnotatedProjectionError::Relation("first-run key overflow"))?,
                "zero first-run key",
            )?,
            run_count: row.run_count,
            column_count: row.column_count,
            break_after: row.break_after,
        });
    }
    Ok(rows)
}

fn metadata(page: &AnnotatedDocument) -> DocumentMeta {
    let native = &page.metadata;
    DocumentMeta {
        title: native.title.clone(),
        manual_section: native.section.clone(),
        date: native.date.clone(),
        volume: native.volume.clone(),
        os: native.operating_system.clone(),
        arch: native.architecture.clone(),
        names: native.name.iter().cloned().collect(),
        alias_target: native.alias_target.clone(),
    }
}

fn mark_source(mark: &AnnotatedMark) -> Result<Option<SourceSpan>> {
    match (mark.source, mark.line, mark.column) {
        (_, 0, 0) => Ok(None),
        (source, line, column) if source != 0 && line != 0 && column != 0 => Ok(Some(SourceSpan {
            source: key_source(source)?,
            byte_range: None,
            line,
            column,
            end_line: None,
            end_column: None,
        })),
        _ => Err(AnnotatedProjectionError::Relation(
            "partial native mark source",
        )),
    }
}

fn point(point: AnnotatedDisplayPoint) -> Result<DisplayPoint> {
    Ok(match point {
        AnnotatedDisplayPoint::RowColumn { row, column } => DisplayPoint::RowColumn {
            row: key(row, "zero display point row")?,
            column,
        },
        AnnotatedDisplayPoint::DocumentEnd { row_count } => DisplayPoint::DocumentEnd { row_count },
    })
}

fn selection(page: &AnnotatedDocument, mark: &AnnotatedMark) -> Result<TextSelection> {
    let first = mark.selection_first as usize;
    let end = first.checked_add(mark.selection_count as usize).ok_or(
        AnnotatedProjectionError::Relation("selection range overflow"),
    )?;
    let native = page
        .selection_parts
        .get(first..end)
        .ok_or(AnnotatedProjectionError::Relation(
            "selection range outside native result",
        ))?;
    let mut parts = Vec::with_capacity(native.len());
    let mut joins = Vec::with_capacity(native.len().saturating_sub(1));
    for (index, part) in native.iter().enumerate() {
        parts.push(OutputSlice {
            run: key(part.run, "zero selection run")?,
            start_byte: part.start_byte,
            end_byte: part.end_byte,
        });
        if index != 0 {
            joins.push(match part.join_before {
                AnnotatedTextJoin::DirectContact => TextJoin::DirectContact,
                AnnotatedTextJoin::AuthoredSeparator | AnnotatedTextJoin::GeneratedSeparator => {
                    let start = usize::try_from(part.join_text_start)
                        .map_err(|_| AnnotatedProjectionError::Relation("join start overflow"))?;
                    let end = start
                        .checked_add(usize::try_from(part.join_text_len).map_err(|_| {
                            AnnotatedProjectionError::Relation("join length overflow")
                        })?)
                        .ok_or(AnnotatedProjectionError::Relation("join text overflow"))?;
                    let text = page
                        .join_text
                        .get(start..end)
                        .ok_or(AnnotatedProjectionError::Relation(
                            "join text outside arena",
                        ))?
                        .to_owned();
                    if part.join_before == AnnotatedTextJoin::AuthoredSeparator {
                        TextJoin::AuthoredSeparator(text)
                    } else {
                        TextJoin::GeneratedSeparator(text)
                    }
                }
                AnnotatedTextJoin::HardBoundary => TextJoin::HardBoundary,
                AnnotatedTextJoin::Unknown => TextJoin::Unknown,
                AnnotatedTextJoin::None => {
                    return Err(AnnotatedProjectionError::Relation("missing native join"));
                }
            });
        } else if part.join_before != AnnotatedTextJoin::None {
            return Err(AnnotatedProjectionError::Relation(
                "first selection has a join",
            ));
        }
    }
    Ok(TextSelection { parts, joins })
}

fn region_selection(page: &AnnotatedDocument, value: u32) -> Result<TextSelection> {
    let region = page
        .marks
        .get(value.saturating_sub(1) as usize)
        .filter(|mark| mark.key == value && mark.kind == 5)
        .ok_or(AnnotatedProjectionError::Relation(
            "missing head/body region",
        ))?;
    selection(page, region)
}

mod identity;
use identity::{Identities, KeyMap};

mod marks;
use marks::{project_anchor, project_heading, project_link, project_owner, project_region};

mod diagnostics;
use diagnostics::{coverage_diagnostics, native_diagnostics};

#[cfg(test)]
mod tests;
