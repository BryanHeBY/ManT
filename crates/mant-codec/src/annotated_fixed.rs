//! Direct projection from the checked native post-device result to Fixed IR.
//!
//! Native owns the only display byte arena. This module changes key domains
//! and closes typed references; it never replays roff or builds Flow prose.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    num::NonZeroU32,
};

use libmandoc_rs::annotated::{
    AnnotatedDisplayPoint, AnnotatedDocument, AnnotatedError, AnnotatedMark, AnnotatedRenderer,
    AnnotatedTextJoin, AnnotationCheckState, AnnotationDimension, AnnotationIssueReason,
    AnnotationScope,
};
use libmandoc_rs::{InputFormat, SourceBundle};
use mant_ir::{
    AnchorMark, CoverageScope, Diagnostic, DiagnosticImpact, DiagnosticLevel, DisplayLabel,
    DisplayPoint, DisplayRole, DisplayRow, DisplayRun, DisplayStyle, DisplaySurface, Document,
    DocumentBody, DocumentMeta, FixedBody, FragmentAlias, HeadingMark, LinkMark, LinkTarget,
    NodeId, OutputSlice, OwnerMark, OwnerRole, ParserInfo, RegionKind, RegionMark,
    SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord, SourceSpan, TextJoin,
    TextSelection, validate_document, validate_document_sources,
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
    for mark in &page.marks {
        match mark.kind {
            1 => headings.push(project_heading(&page, &keys, &identities, mark)?),
            2 => owners.push(project_owner(&page, &keys, &identities, mark)?),
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
            4 => {}
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
    let fixed = FixedBody {
        surface,
        headings,
        owners,
        links,
        anchors,
        regions,
    };
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
                AnnotatedTextJoin::AuthoredSeparator => {
                    let start = usize::try_from(part.join_text_start)
                        .map_err(|_| AnnotatedProjectionError::Relation("join start overflow"))?;
                    let end = start
                        .checked_add(usize::try_from(part.join_text_len).map_err(|_| {
                            AnnotatedProjectionError::Relation("join length overflow")
                        })?)
                        .ok_or(AnnotatedProjectionError::Relation("join text overflow"))?;
                    TextJoin::AuthoredSeparator(
                        page.join_text
                            .get(start..end)
                            .ok_or(AnnotatedProjectionError::Relation(
                                "join text outside arena",
                            ))?
                            .to_owned(),
                    )
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

struct KeyMap {
    heading: Vec<u32>,
    owner: Vec<u32>,
    link: Vec<u32>,
    anchor: Vec<u32>,
    region: Vec<u32>,
    // Inclusive nearest typed key for heading, owner, and region. Native marks
    // are preorder, so each row is derived once from its earlier parent.
    nearest: Vec<[u32; 3]>,
}

struct Identities {
    heading: Vec<Option<NodeId>>,
    owner: Vec<Option<NodeId>>,
    anchor: Vec<Option<NodeId>>,
    heading_aliases: Vec<Vec<FragmentAlias>>,
    heading_generated_aliases: Vec<Vec<FragmentAlias>>,
    heading_rendered_aliases: Vec<Vec<FragmentAlias>>,
    anchor_rendered: Vec<Option<FragmentAlias>>,
    consumed_anchor: Vec<bool>,
    section_targets: crate::mandoc::navigation::SectionTargets,
}

impl Identities {
    #[allow(clippy::too_many_lines)] // One collision domain for heading, anchor and owner IDs.
    fn new(page: &AnnotatedDocument) -> Result<Self> {
        let mut used = HashSet::new();
        let mut next_suffix = HashMap::new();
        used.insert(mant_ir::DOCUMENT_ROOT_ID.to_owned());
        let length = page
            .marks
            .len()
            .checked_add(1)
            .ok_or(AnnotatedProjectionError::Relation("mark count overflow"))?;
        // This owned input is public and may not have crossed the native FFI
        // checker. Validate the global preorder keys before using any key as
        // a vector index or as the carrier of a moved tag.c target.
        for (index, mark) in page.marks.iter().enumerate() {
            if mark.key as usize != index + 1
                || !(1..=5).contains(&mark.kind)
                || (mark.parent != 0 && mark.parent >= mark.key)
            {
                return Err(AnnotatedProjectionError::Relation(
                    "invalid global native mark key or parent",
                ));
            }
        }
        let mut heading = vec![None; length];
        let mut owner = vec![None; length];
        let mut anchor = vec![None; length];
        let mut heading_aliases = vec![Vec::new(); length];
        let mut heading_generated_aliases = vec![Vec::new(); length];
        let mut heading_rendered_aliases = vec![Vec::new(); length];
        let mut anchor_rendered = vec![None; length];
        let mut consumed_anchor = vec![false; length];
        let mut section_targets = crate::mandoc::navigation::SectionTargets::new();

        // A surviving tag on a heading HEAD is the heading's own target,
        // not an independent anchor.  The native parent points at that exact
        // HEAD region; no display-neighbor or prior-root guess is involved.
        let mut carrier = vec![0_u32; length];
        for mark in &page.marks {
            if mark.kind != 1 {
                continue;
            }
            if mark.title_region != 0 {
                *carrier.get_mut(mark.title_region as usize).ok_or(
                    AnnotatedProjectionError::Relation("heading title region outside mark table"),
                )? = mark.key;
            }
        }
        // Native HTML emits one unique ID for each surviving NODE_ID, in
        // preorder. Keep the original declaration separately from that ID.
        // A canonical slug may equal its own emitted target, but never
        // another target's fragment.
        let mut target_claims = HashMap::<String, HashSet<u32>>::new();
        let mut html_ordinals = HashMap::<String, u64>::new();
        for mark in &page.marks {
            if mark.kind != 4 {
                continue;
            }
            let name = mark
                .name
                .as_deref()
                .ok_or(AnnotatedProjectionError::Relation("anchor has no name"))?;
            let rendered = render_html_fragment(name, &mut html_ordinals)?;
            let heading_key = carrier.get(mark.parent as usize).copied().unwrap_or(0);
            let attached =
                heading_key != 0 && page.marks[(heading_key - 1) as usize].token == mark.token;
            if attached {
                consumed_anchor[mark.key as usize] = true;
                if valid_alias(name) {
                    if mark.flags & 4 != 0 {
                        heading_aliases[heading_key as usize].push(FragmentAlias::from(name));
                    } else {
                        heading_generated_aliases[heading_key as usize]
                            .push(FragmentAlias::from(name));
                    }
                    heading_rendered_aliases[heading_key as usize]
                        .push(FragmentAlias::from(rendered.as_str()));
                    target_claims
                        .entry(rendered)
                        .or_default()
                        .insert(heading_key);
                }
            } else if valid_alias(name) {
                anchor_rendered[mark.key as usize] = Some(FragmentAlias::from(rendered.as_str()));
                target_claims.entry(rendered).or_default().insert(mark.key);
            }
        }
        // The authored phrase is captured from the final AST HEAD with the
        // pinned roff.c::deroff rule.  Final display spelling can differ after
        // overprint, deletion, or an unknown text join and is never an ID.
        for mark in &page.marks {
            if mark.kind != 1 {
                continue;
            }
            let base = mark.name.as_deref().map_or_else(
                || "section".to_owned(),
                |phrase| identity_base(phrase, "section", "section"),
            );
            // An authored alias on a *different* heading, or on an independent
            // anchor, owns its exact fragment spelling.  The current
            // heading's own alias may also be its canonical normalized ID.
            let id = allocate_id_excluding(&base, &mut used, &mut next_suffix, |candidate| {
                target_claims
                    .get(candidate)
                    .is_some_and(|owners| owners.iter().any(|owner| *owner != mark.key))
            })?;
            if let Some(phrase) = &mark.name {
                section_targets
                    .entry(phrase.clone())
                    .and_modify(|target| *target = None)
                    .or_insert_with(|| Some(id.clone()));
            }
            *heading
                .get_mut(mark.key as usize)
                .ok_or(AnnotatedProjectionError::Relation(
                    "heading identity key outside mark table",
                ))? = Some(NodeId::new(id));
        }
        for mark in &page.marks {
            if mark.kind != 4 || consumed_anchor[mark.key as usize] {
                continue;
            }
            let name = mark
                .name
                .as_deref()
                .ok_or(AnnotatedProjectionError::Relation("anchor has no name"))?;
            let base = identity_base(name, "anchor", "anchor");
            let id = allocate_id_excluding(&base, &mut used, &mut next_suffix, |candidate| {
                target_claims
                    .get(candidate)
                    .is_some_and(|owners| owners.iter().any(|owner| *owner != mark.key))
            })?;
            anchor[mark.key as usize] = Some(NodeId::new(id));
        }
        for mark in &page.marks {
            if mark.kind != 2 {
                continue;
            }
            let base = format!("native-owner-{}", mark.key);
            let id = allocate_id_excluding(&base, &mut used, &mut next_suffix, |candidate| {
                target_claims.contains_key(candidate)
            })?;
            owner[mark.key as usize] = Some(NodeId::new(id));
        }
        Ok(Self {
            heading,
            owner,
            anchor,
            heading_aliases,
            heading_generated_aliases,
            heading_rendered_aliases,
            anchor_rendered,
            consumed_anchor,
            section_targets,
        })
    }

    fn heading(&self, global: u32) -> Result<NodeId> {
        self.heading
            .get(global as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "heading identity missing",
            ))
    }

    fn owner(&self, global: u32) -> Result<NodeId> {
        self.owner
            .get(global as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation("owner identity missing"))
    }

    fn anchor(&self, global: u32) -> Result<NodeId> {
        self.anchor
            .get(global as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "anchor identity missing",
            ))
    }

    fn heading_aliases(&self, global: u32) -> Result<Vec<FragmentAlias>> {
        self.heading_aliases.get(global as usize).cloned().ok_or(
            AnnotatedProjectionError::Relation("heading alias key missing"),
        )
    }

    fn heading_generated_aliases(&self, global: u32) -> Result<Vec<FragmentAlias>> {
        self.heading_generated_aliases
            .get(global as usize)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "heading generated alias key missing",
            ))
    }

    fn heading_rendered_aliases(&self, global: u32) -> Result<Vec<FragmentAlias>> {
        self.heading_rendered_aliases
            .get(global as usize)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "heading rendered alias key missing",
            ))
    }

    fn anchor_rendered(&self, global: u32) -> Result<FragmentAlias> {
        self.anchor_rendered
            .get(global as usize)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or(AnnotatedProjectionError::Relation(
                "anchor rendered fragment missing",
            ))
    }

    fn consumed_anchor(&self, global: u32) -> Result<bool> {
        self.consumed_anchor
            .get(global as usize)
            .copied()
            .ok_or(AnnotatedProjectionError::Relation("anchor key missing"))
    }

    fn section(&self, phrase: &str) -> Option<NodeId> {
        crate::mandoc::navigation::resolve_section_target(&self.section_targets, phrase)
            .map(NodeId::new)
    }
}

fn valid_alias(value: &str) -> bool {
    !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

// Pinned CVS html.c::html_make_id: sanitize each byte, then reserve '~' for
// the ordinal suffix. The annotated marks are emitted in native AST preorder.
fn render_html_fragment(name: &str, ordinals: &mut HashMap<String, u64>) -> Result<String> {
    let mut base = String::with_capacity(name.len());
    for byte in name.bytes() {
        match byte {
            28 => base.push('-'), // mandoc.h::ASCII_HYPH
            b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => base.push(char::from(byte)),
            b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b'-' | b'.' | b'/'
            | b':' | b';' | b'=' | b'?' | b'@' | b'_' => {
                base.push(char::from(byte));
            }
            _ => base.push('_'),
        }
    }
    let ordinal = ordinals.entry(base.clone()).or_insert(0);
    *ordinal = ordinal
        .checked_add(1)
        .ok_or(AnnotatedProjectionError::Relation(
            "HTML ID ordinal overflow",
        ))?;
    Ok(if *ordinal == 1 {
        base
    } else {
        format!("{base}~{ordinal}")
    })
}

fn identity_base(value: &str, fallback: &str, reserved_suffix: &str) -> String {
    // Same grammar and reserved-selector rule as the existing native Flow
    // address planner; Fixed uses its final title selection as the phrase.
    let has_identity_character =
        value.chars().any(char::is_alphanumeric) || value.trim_start_matches(['-', '/']) == "?";
    let base = if has_identity_character {
        crate::definitions::document_id_slug(value)
    } else {
        fallback.to_owned()
    };
    if crate::producer_identity::is_reserved_selector(&base) {
        format!("{base}-{reserved_suffix}")
    } else {
        base
    }
}

fn allocate_id_excluding(
    base: &str,
    used: &mut HashSet<String>,
    next_suffix: &mut HashMap<String, u64>,
    mut excluded: impl FnMut(&str) -> bool,
) -> Result<String> {
    let mut suffix = next_suffix.get(base).copied().unwrap_or(1);
    loop {
        let candidate = if suffix == 1 {
            base.to_owned()
        } else {
            format!("{base}-{suffix}")
        };
        if !excluded(&candidate) && used.insert(candidate.clone()) {
            next_suffix.insert(
                base.to_owned(),
                suffix
                    .checked_add(1)
                    .ok_or(AnnotatedProjectionError::Relation(
                        "identity suffix overflow",
                    ))?,
            );
            return Ok(candidate);
        }
        suffix = suffix
            .checked_add(1)
            .ok_or(AnnotatedProjectionError::Relation(
                "identity suffix overflow",
            ))?;
    }
}

impl KeyMap {
    fn new(page: &AnnotatedDocument, identities: &Identities) -> Result<Self> {
        let length = page
            .marks
            .len()
            .checked_add(1)
            .ok_or(AnnotatedProjectionError::Relation("mark count overflow"))?;
        let mut map = Self {
            heading: vec![0; length],
            owner: vec![0; length],
            link: vec![0; length],
            anchor: vec![0; length],
            region: vec![0; length],
            nearest: vec![[0; 3]; length],
        };
        let mut counts = [0_u32; 5];
        for (index, mark) in page.marks.iter().enumerate() {
            if mark.key as usize != index + 1 || !(1..=5).contains(&mark.kind) {
                return Err(AnnotatedProjectionError::Relation(
                    "invalid global native mark key",
                ));
            }
            // Public owned results can be constructed without crossing the
            // FFI checker. Keep role evidence closed at this boundary too.
            let allowed_flags = match mark.kind {
                1 => 0b1001,     // authored heading and subsection
                2 => 0b1_0001,   // authored owner and definition
                4 => 0b0101,     // authored anchor and manual target
                3 | 5 => 0b0001, // authored link or region
                _ => unreachable!(),
            };
            if mark.flags & !allowed_flags != 0 {
                return Err(AnnotatedProjectionError::Relation(
                    "native mark has invalid kind flags",
                ));
            }
            // Native marks form a preorder forest. Public owned results can
            // also be constructed by callers, so reject cycles before any
            // ancestry lookup instead of relying on the FFI validator.
            if mark.parent >= mark.key && mark.parent != 0 {
                return Err(AnnotatedProjectionError::Relation(
                    "native mark parent does not precede child",
                ));
            }
            if mark.kind == 4 && identities.consumed_anchor(mark.key)? {
                // tag.c::tag_move_id can make a manual target the heading's
                // authored alias. The original global mark stays in native
                // identity order, but it is not a separate Fixed anchor.
                map.nearest[mark.key as usize] = map.nearest[mark.parent as usize];
                continue;
            }
            let slot = (mark.kind - 1) as usize;
            counts[slot] =
                counts[slot]
                    .checked_add(1)
                    .ok_or(AnnotatedProjectionError::Relation(
                        "typed mark key overflow",
                    ))?;
            let target = match mark.kind {
                1 => &mut map.heading,
                2 => &mut map.owner,
                3 => &mut map.link,
                4 => &mut map.anchor,
                5 => &mut map.region,
                _ => unreachable!(),
            };
            *target
                .get_mut(mark.key as usize)
                .ok_or(AnnotatedProjectionError::Relation(
                    "native mark key outside mark table",
                ))? = counts[slot];
            let parent = map.nearest[mark.parent as usize];
            let mut nearest = parent;
            match mark.kind {
                1 => nearest[0] = counts[0],
                2 => nearest[1] = counts[1],
                5 => nearest[2] = counts[4],
                _ => {}
            }
            map.nearest[mark.key as usize] = nearest;
        }
        Ok(map)
    }

    fn lookup(&self, global: u32, kind: u32) -> Result<Option<NonZeroU32>> {
        if global == 0 {
            return Ok(None);
        }
        let domain = match kind {
            1 => &self.heading,
            2 => &self.owner,
            3 => &self.link,
            4 => &self.anchor,
            5 => &self.region,
            _ => {
                return Err(AnnotatedProjectionError::Relation(
                    "unknown typed key domain",
                ));
            }
        };
        let value = domain.get(global as usize).copied().unwrap_or(0);
        Ok(Some(key(value, "global mark has wrong typed key domain")?))
    }

    fn required(&self, global: u32, kind: u32) -> Result<NonZeroU32> {
        self.lookup(global, kind)?
            .ok_or(AnnotatedProjectionError::Relation("missing typed mark key"))
    }

    fn nearest(&self, global: u32, kind: u32) -> Result<Option<NonZeroU32>> {
        let slot = match kind {
            1 => 0,
            2 => 1,
            5 => 2,
            _ => return Err(AnnotatedProjectionError::Relation("unknown ancestry kind")),
        };
        let value = self
            .nearest
            .get(global as usize)
            .ok_or(AnnotatedProjectionError::Relation(
                "mark ancestry is broken",
            ))?[slot];
        Ok(NonZeroU32::new(value))
    }
}

fn project_heading(
    page: &AnnotatedDocument,
    keys: &KeyMap,
    identities: &Identities,
    mark: &AnnotatedMark,
) -> Result<HeadingMark> {
    let title = region_selection(page, mark.title_region)?;
    let region = page
        .marks
        .get(mark.title_region.saturating_sub(1) as usize)
        .filter(|region| region.key == mark.title_region && region.kind == 5)
        .ok_or(AnnotatedProjectionError::Relation(
            "missing heading title region",
        ))?;
    // man_term.c/mdoc_term.c emit the region point after the heading pre
    // handler and before its child output. Keep that structural boundary if
    // it precedes the first visible title run, including native indentation.
    // A point inside a coalesced run cannot be converted from terminal cells
    // to UTF-8 bytes, so that case uses the exact first title byte instead.
    let native_at = point(region.point.ok_or(AnnotatedProjectionError::Relation(
        "heading title has no final point",
    ))?)?;
    let at = if let Some(first) = title.parts.first() {
        let run = page.runs.get((first.run.get() - 1) as usize).ok_or(
            AnnotatedProjectionError::Relation("heading title run is missing"),
        )?;
        let run_index = first.run.get() - 1;
        let row_index = page
            .rows
            .partition_point(|row| row.first_run.saturating_add(row.run_count) <= run_index);
        let run_row = page
            .rows
            .get(row_index)
            .ok_or(AnnotatedProjectionError::Relation(
                "heading title run has no row",
            ))?;
        if run_index < run_row.first_run {
            return Err(AnnotatedProjectionError::Relation(
                "heading title run has no row",
            ));
        }
        match native_at {
            DisplayPoint::RowColumn { row, column }
                if row.get() < run_row.key
                    || (row.get() == run_row.key && column <= run.column) =>
            {
                native_at
            }
            DisplayPoint::RowColumn { row, .. } if row.get() <= run_row.key => {
                DisplayPoint::RunBoundary {
                    run: first.run,
                    byte: first.start_byte,
                }
            }
            _ => {
                return Err(AnnotatedProjectionError::Relation(
                    "heading point follows title",
                ));
            }
        }
    } else {
        native_at
    };
    Ok(HeadingMark {
        key: keys.required(mark.key, 1)?,
        id: identities.heading(mark.key)?,
        fragment_aliases: identities.heading_aliases(mark.key)?,
        generated_fragment_aliases: identities.heading_generated_aliases(mark.key)?,
        rendered_fragment_aliases: identities.heading_rendered_aliases(mark.key)?,
        parent: keys.lookup(mark.parent, 1)?,
        // MAN_SS/MDOC_Ss remain subsections even before any top heading.
        level_hint: if mark.flags & 8 != 0 { 2 } else { 1 },
        at,
        title,
        direct_body: region_selection(page, mark.body_region)?,
        source: mark_source(mark)?,
    })
}

fn project_owner(
    page: &AnnotatedDocument,
    keys: &KeyMap,
    identities: &Identities,
    mark: &AnnotatedMark,
) -> Result<OwnerMark> {
    let head = region_selection(page, mark.title_region)?;
    let direct_body = region_selection(page, mark.body_region)?;
    let empty_point = if head.parts.is_empty() && direct_body.parts.is_empty() {
        Some(point(mark.point.ok_or(
            AnnotatedProjectionError::Relation("empty owner has no final point"),
        )?)?)
    } else {
        None
    };
    Ok(OwnerMark {
        key: keys.required(mark.key, 2)?,
        id: identities.owner(mark.key)?,
        parent: keys.nearest(mark.parent, 2)?,
        section: keys.nearest(mark.parent, 1)?,
        role: if mark.flags & 16 != 0 {
            OwnerRole::Definition
        } else {
            OwnerRole::Other
        },
        head,
        direct_body,
        empty_point,
        source: mark_source(mark)?,
    })
}

fn project_link(
    page: &AnnotatedDocument,
    keys: &KeyMap,
    identities: &Identities,
    mark: &AnnotatedMark,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<LinkMark> {
    let authored = mark_source(mark)?;
    let target = match &mark.link_target {
        None => None,
        Some(native) => match native.kind {
            1 => Some(LinkTarget::External {
                uri: native.primary.clone(),
            }),
            2 => Some(LinkTarget::Email {
                address: native.primary.clone(),
            }),
            3 => Some(LinkTarget::Document {
                name: native.primary.clone(),
                fragment: native.secondary.clone(),
            }),
            4 => Some(LinkTarget::Manual {
                name: native.primary.clone(),
                manual_section: native.secondary.clone(),
            }),
            5 => identities
                .section(&native.primary)
                .map(|id| LinkTarget::Section { id })
                .or_else(|| {
                    // The native occurrence and final label remain. A missing or
                    // ambiguous local destination is not made clickable and does
                    // not invent a canonical ID, matching Flow's AddressPlan.
                    diagnostics.push(Diagnostic {
                        level: DiagnosticLevel::Warning,
                        impact: DiagnosticImpact::None,
                        code: Some("unresolved-section-reference".to_owned()),
                        message: format!("cannot resolve section reference: {}", native.primary),
                        source: authored,
                        coverage_scope: None,
                    });
                    None
                }),
            _ => {
                return Err(AnnotatedProjectionError::Relation(
                    "unknown native link target",
                ));
            }
        },
    };
    Ok(LinkMark {
        key: keys.required(mark.key, 3)?,
        target,
        label: selection(page, mark)?,
        source: authored,
    })
}

fn project_anchor(
    keys: &KeyMap,
    identities: &Identities,
    mark: &AnnotatedMark,
) -> Result<AnchorMark> {
    Ok(AnchorMark {
        key: keys.required(mark.key, 4)?,
        id: identities.anchor(mark.key)?,
        section: keys.nearest(mark.parent, 1)?,
        name: mark
            .name
            .clone()
            .ok_or(AnnotatedProjectionError::Relation("anchor has no name"))?,
        rendered_fragment: identities.anchor_rendered(mark.key)?,
        authored: mark.flags & 4 != 0,
        at: point(mark.point.ok_or(AnnotatedProjectionError::Relation(
            "anchor has no final point",
        ))?)?,
        source: mark_source(mark)?,
    })
}

fn project_region(
    page: &AnnotatedDocument,
    keys: &KeyMap,
    mark: &AnnotatedMark,
) -> Result<RegionMark> {
    let selection = selection(page, mark)?;
    let empty_point = if selection.parts.is_empty() {
        Some(point(mark.point.ok_or(
            AnnotatedProjectionError::Relation("empty region has no final point"),
        )?)?)
    } else {
        None
    };
    Ok(RegionMark {
        key: keys.required(mark.key, 5)?,
        parent: keys.nearest(mark.parent, 5)?,
        owner: keys.nearest(mark.owner, 2)?,
        kind: match mark.region_kind {
            10 => RegionKind::Unsectioned,
            1 => RegionKind::HeadingTitle,
            2 => RegionKind::HeadingBody,
            3 => RegionKind::OwnerHead,
            4 => RegionKind::OwnerBody,
            5 => RegionKind::List,
            6 => RegionKind::Literal,
            7 => RegionKind::TableSpan,
            8 => RegionKind::Equation,
            9 => RegionKind::TableCell,
            _ => {
                return Err(AnnotatedProjectionError::Relation(
                    "unknown native region kind",
                ));
            }
        },
        selection,
        empty_point,
        source: mark_source(mark)?,
    })
}

fn native_diagnostics(page: &AnnotatedDocument) -> Result<Vec<Diagnostic>> {
    page.diagnostics
        .iter()
        .map(|native| {
            let source = if native.span == 0 {
                None
            } else {
                let span = page.spans.get((native.span - 1) as usize).ok_or(
                    AnnotatedProjectionError::Relation("diagnostic span missing"),
                )?;
                span.line_column
                    .map(
                        |(line, column, end_line, end_column)| -> Result<SourceSpan> {
                            Ok(SourceSpan {
                                source: key_source(span.source)?,
                                byte_range: None,
                                line,
                                column,
                                end_line: (end_line != 0).then_some(end_line),
                                end_column: (end_column != 0).then_some(end_column),
                            })
                        },
                    )
                    .transpose()?
            };
            Ok(Diagnostic {
                level: match native.level {
                    1 => DiagnosticLevel::Style,
                    2 => DiagnosticLevel::Warning,
                    3 => DiagnosticLevel::Error,
                    4 => DiagnosticLevel::Unsupported,
                    _ => {
                        return Err(AnnotatedProjectionError::Relation(
                            "unknown native diagnostic level",
                        ));
                    }
                },
                impact: DiagnosticImpact::None,
                code: Some(format!("mandoc.native-{}", native.code)),
                message: native.message.clone(),
                source,
                coverage_scope: None,
            })
        })
        .collect()
}

fn coverage_diagnostics(page: &AnnotatedDocument, keys: &KeyMap) -> Result<Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    for check in &page.coverage.checks {
        if check.state == AnnotationCheckState::Pending {
            diagnostics.push(coverage_diagnostic(
                check.dimension,
                "unverified",
                CoverageScope::Document,
                None,
            ));
        }
    }
    for issue in &page.coverage.issues {
        let scope = match issue.scope {
            AnnotationScope::Document => CoverageScope::Document,
            AnnotationScope::Section(global) => CoverageScope::Section {
                key: keys.required(global, 1)?,
            },
            AnnotationScope::Owner(global) => CoverageScope::Owner {
                key: keys.required(global, 2)?,
            },
            AnnotationScope::Region(global) => CoverageScope::Region {
                key: keys.required(global, 5)?,
            },
            AnnotationScope::Source(raw) => CoverageScope::Source {
                key: key_source(raw)?,
            },
        };
        let source = issue
            .source
            .map(|position| -> Result<SourceSpan> {
                Ok(SourceSpan {
                    source: key_source(position.source)?,
                    byte_range: None,
                    line: position.line,
                    column: position.column,
                    end_line: None,
                    end_column: None,
                })
            })
            .transpose()?;
        let reason = match issue.reason {
            AnnotationIssueReason::NotObserved => "not-observed",
            AnnotationIssueReason::Unverified => "unverified",
            AnnotationIssueReason::Rejected => "rejected",
            AnnotationIssueReason::AmbiguousSurvival => "ambiguous-survival",
        };
        diagnostics.push(coverage_diagnostic(issue.dimension, reason, scope, source));
    }
    Ok(diagnostics)
}

fn coverage_diagnostic(
    dimension: AnnotationDimension,
    reason: &str,
    coverage_scope: CoverageScope,
    source: Option<SourceSpan>,
) -> Diagnostic {
    let dimension = match dimension {
        AnnotationDimension::Section => "section",
        AnnotationDimension::OwnerBoundary => "owner-boundary",
        AnnotationDimension::Declaration => "declaration",
        AnnotationDimension::Link => "link",
        AnnotationDimension::Anchor => "anchor",
        AnnotationDimension::Relation => "relation",
        AnnotationDimension::Source => "source",
        AnnotationDimension::Join => "join",
    };
    Diagnostic {
        level: DiagnosticLevel::Unsupported,
        impact: DiagnosticImpact::SemanticCoverage,
        code: Some(format!("annotated.coverage.{dimension}.{reason}")),
        message: format!("{dimension} evidence {reason}"),
        source,
        coverage_scope: Some(coverage_scope),
    }
}

#[cfg(test)]
mod tests;
